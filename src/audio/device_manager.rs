//! The device manager thread. It owns every cpal stream, re-scans devices every 1.5 s, notices
//! dead or stalled streams, reopens them, and decides which mic is live. None of this happens
//! on an audio thread, so it may block, allocate and take locks freely.
//!
//! Lifetimes: the output stream (virtual mic) carries the [`Engine`]. Mic and monitor streams
//! are attached to the running engine through its command queue and can come and go without
//! touching the output. Only changing the output device or buffer size restarts everything.

use super::adaptive::{AdaptiveReader, target_fill};
use super::block::BLOCK;
use super::engine::{Engine, EngineCmd, Garbage, MonitorSource};
use super::matching::{best_match, find_cable_input};
use super::mic_select::{MicInputs, MicSelector, MicSettings};
use super::shared::{ActiveMic, Shared, to_db};
use super::streams::{self, Opened, StreamHealth};
use cpal::BufferSize;
use cpal::traits::HostTrait;
use rtrb::{Consumer, Producer, RingBuffer};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(50);
const SCAN_EVERY: Duration = Duration::from_millis(1500);
const RETRY_AFTER: Duration = Duration::from_secs(2);
/// A stream whose callback has not run for this long is treated as dead.
const STALL_AFTER: Duration = Duration::from_millis(1500);
/// No "switched mic" notifications while everything is still opening.
const STARTUP_GRACE: Duration = Duration::from_millis(2500);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceSettings {
    /// The virtual mic, normally "CABLE Input (VB-Audio Virtual Cable)".
    pub output: String,
    pub monitor: String,
    pub monitor_enabled: bool,
    pub primary: String,
    pub fallback: String,
    /// Requested frames per device callback; 0 = device default.
    pub buffer_frames: u32,
}

pub enum ManagerCmd {
    Devices(DeviceSettings),
    Mic(MicSettings),
    Rescan,
    Quit,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum LinkState {
    /// Nothing selected.
    #[default]
    Off,
    /// Selected but not connected.
    Missing,
    /// Connected, but can only start once the virtual mic output is running.
    Waiting,
    Running,
    /// Running but silent for too long (primary mic only).
    Silent,
    Error(String),
}

#[derive(Clone, Debug, Default)]
pub struct Endpoint {
    /// The actual device name matched for the configured one.
    pub device: Option<String>,
    pub state: LinkState,
    pub rate: u32,
    pub period: u32,
}

#[derive(Clone, Debug, Default)]
pub struct DeviceStatus {
    /// False until the first device scan finished.
    pub scanned: bool,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub default_input: Option<String>,
    /// The VB-CABLE playback device, if installed.
    pub cable: Option<String>,
    pub output: Endpoint,
    pub monitor: Endpoint,
    pub mics: [Endpoint; 2],
    pub active: ActiveMic,
    /// Why the active mic is what it is ("primary not available").
    pub reason: String,
    /// Estimated mic-to-virtual-mic latency.
    pub latency_ms: Option<f32>,
}

pub enum Event {
    Switched { to: ActiveMic, reason: String },
}

/// Sender into whichever engine is currently running. Only non-audio threads lock this.
#[derive(Default)]
pub struct EnginePort(Mutex<Option<Producer<EngineCmd>>>);

impl EnginePort {
    /// False if no engine is running or its queue is full.
    pub fn send(&self, cmd: EngineCmd) -> bool {
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_mut().is_some_and(|tx| tx.push(cmd).is_ok())
    }

    pub(crate) fn set(&self, tx: Option<Producer<EngineCmd>>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = tx;
    }
}

pub struct ManagerHandle {
    tx: mpsc::Sender<ManagerCmd>,
    status: Arc<Mutex<DeviceStatus>>,
    pub events: mpsc::Receiver<Event>,
    pub port: Arc<EnginePort>,
    thread: Option<JoinHandle<()>>,
}

impl ManagerHandle {
    pub fn spawn(shared: Arc<Shared>, devices: DeviceSettings, mic: MicSettings) -> Self {
        let (tx, rx) = mpsc::channel();
        let (ev_tx, ev_rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(DeviceStatus::default()));
        let port = Arc::new(EnginePort::default());
        let (st, pt) = (status.clone(), port.clone());
        let thread = std::thread::Builder::new()
            .name("device-manager".into())
            .spawn(move || Manager::new(shared, st, ev_tx, pt, devices, mic).run(rx))
            .expect("spawn device manager");
        Self {
            tx,
            status,
            events: ev_rx,
            port,
            thread: Some(thread),
        }
    }

    /// A handle with no thread behind it (for UI tests: opens no devices).
    #[cfg(test)]
    pub fn detached(status: DeviceStatus) -> Self {
        let (tx, _) = mpsc::channel();
        let (_, events) = mpsc::channel();
        Self {
            tx,
            status: Arc::new(Mutex::new(status)),
            events,
            port: Arc::new(EnginePort::default()),
            thread: None,
        }
    }

    pub fn send(&self, cmd: ManagerCmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn status(&self) -> DeviceStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// The live status, for other threads (the tray) to read.
    pub fn status_handle(&self) -> Arc<Mutex<DeviceStatus>> {
        self.status.clone()
    }

    /// Stops every stream and the manager thread. Safe to call more than once.
    pub fn shutdown(&mut self) {
        if let Some(t) = self.thread.take() {
            let _ = self.tx.send(ManagerCmd::Quit);
            let _ = t.join();
        }
    }
}

impl Drop for ManagerHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Tracks whether a stream's callback keeps running.
struct Watchdog {
    last: u32,
    since: Instant,
}

impl Watchdog {
    fn new(now: Instant) -> Self {
        Self {
            last: 0,
            since: now,
        }
    }

    fn stalled(&mut self, health: &StreamHealth, now: Instant) -> bool {
        let hb = health.heartbeat();
        if hb != self.last {
            self.last = hb;
            self.since = now;
            false
        } else {
            now.duration_since(self.since) > STALL_AFTER
        }
    }
}

struct Live {
    opened: Opened,
    name: String,
    watchdog: Watchdog,
}

struct Output {
    live: Live,
    garbage: Consumer<Garbage>,
}

struct Slot {
    live: Option<Live>,
    state: LinkState,
    retry_at: Instant,
}

impl Slot {
    fn new(now: Instant) -> Self {
        Self {
            live: None,
            state: LinkState::Off,
            retry_at: now,
        }
    }
}

struct Manager {
    host: cpal::Host,
    shared: Arc<Shared>,
    status: Arc<Mutex<DeviceStatus>>,
    events: mpsc::Sender<Event>,
    port: Arc<EnginePort>,
    devices: DeviceSettings,
    mic: MicSettings,

    inputs: Vec<(String, cpal::Device)>,
    outputs: Vec<(String, cpal::Device)>,
    default_input: Option<String>,
    scanned: bool,
    last_scan: Instant,
    rescan: bool,

    output: Option<Output>,
    output_slot: Slot,
    monitor: Slot,
    mics: [Slot; 2],

    selector: MicSelector,
    active: ActiveMic,
    reason: String,
    last_tick: Instant,
    started: Instant,
}

fn buffer_choices(frames: u32) -> Vec<BufferSize> {
    if frames == 0 {
        vec![BufferSize::Default]
    } else {
        // Shared-mode WASAPI often only accepts its own period: fall back to that.
        vec![BufferSize::Fixed(frames), BufferSize::Default]
    }
}

fn names(list: &[(String, cpal::Device)]) -> Vec<String> {
    list.iter().map(|(n, _)| n.clone()).collect()
}

fn find<'a>(list: &'a [(String, cpal::Device)], name: &str) -> Option<&'a cpal::Device> {
    list.iter().find(|(n, _)| n == name).map(|(_, d)| d)
}

impl Manager {
    fn new(
        shared: Arc<Shared>,
        status: Arc<Mutex<DeviceStatus>>,
        events: mpsc::Sender<Event>,
        port: Arc<EnginePort>,
        devices: DeviceSettings,
        mic: MicSettings,
    ) -> Self {
        let now = Instant::now();
        Self {
            host: cpal::default_host(),
            shared,
            status,
            events,
            port,
            devices,
            mic,
            inputs: Vec::new(),
            outputs: Vec::new(),
            default_input: None,
            scanned: false,
            last_scan: now,
            rescan: true,
            output: None,
            output_slot: Slot::new(now),
            monitor: Slot::new(now),
            mics: [Slot::new(now), Slot::new(now)],
            selector: MicSelector::default(),
            active: ActiveMic::None,
            reason: String::new(),
            last_tick: now,
            started: now,
        }
    }

    fn run(mut self, rx: mpsc::Receiver<ManagerCmd>) {
        loop {
            let first = match rx.recv_timeout(TICK) {
                Ok(cmd) => Some(cmd),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            let mut quit = false;
            for cmd in first
                .into_iter()
                .chain(std::iter::from_fn(|| rx.try_recv().ok()))
            {
                quit |= !self.handle(cmd);
            }
            if quit {
                break;
            }
            self.tick(Instant::now());
        }
        self.close_output();
    }

    /// Returns false on quit.
    fn handle(&mut self, cmd: ManagerCmd) -> bool {
        let now = Instant::now();
        match cmd {
            ManagerCmd::Devices(new) => {
                let old = std::mem::replace(&mut self.devices, new);
                let d = &self.devices;
                if old.output != d.output || old.buffer_frames != d.buffer_frames {
                    self.close_output();
                    self.output_slot.retry_at = now;
                }
                let d = &self.devices;
                if old.monitor != d.monitor
                    || old.monitor_enabled != d.monitor_enabled
                    || old.buffer_frames != d.buffer_frames
                {
                    self.close_monitor();
                    self.monitor.retry_at = now;
                }
                if old.primary != self.devices.primary {
                    self.close_mic(0);
                    self.mics[0].retry_at = now;
                }
                if old.fallback != self.devices.fallback {
                    self.close_mic(1);
                    self.mics[1].retry_at = now;
                }
            }
            ManagerCmd::Mic(m) => {
                if m.silence_db != self.mic.silence_db || m.silence_secs != self.mic.silence_secs {
                    self.selector.reset_silence();
                }
                self.mic = m;
            }
            ManagerCmd::Rescan => self.rescan = true,
            ManagerCmd::Quit => return false,
        }
        true
    }

    fn tick(&mut self, now: Instant) {
        if self.rescan || now.duration_since(self.last_scan) >= SCAN_EVERY {
            self.scan(now);
        }
        self.check_output(now);
        self.check_monitor(now);
        for slot in 0..2 {
            self.check_mic(slot, now);
        }
        self.select_mic(now);
        self.collect_garbage();
        self.publish();
        self.last_tick = now;
    }

    fn scan(&mut self, now: Instant) {
        self.rescan = false;
        self.last_scan = now;
        // A failed enumeration keeps the old lists rather than "unplugging" everything.
        if let Ok(devs) = self.host.input_devices() {
            self.inputs = devs.map(|d| (d.to_string(), d)).collect();
        }
        if let Ok(devs) = self.host.output_devices() {
            self.outputs = devs.map(|d| (d.to_string(), d)).collect();
        }
        self.default_input = self.host.default_input_device().map(|d| d.to_string());
        self.scanned = true;
    }

    // ---- output (virtual mic) ----

    fn check_output(&mut self, now: Instant) {
        let output_names = names(&self.outputs);
        if let Some(out) = &mut self.output {
            let reason = if out.live.opened.health.failed() {
                Some("device error")
            } else if !output_names.contains(&out.live.name) {
                Some("device disconnected")
            } else if out.live.watchdog.stalled(&out.live.opened.health, now) {
                Some("device stopped responding")
            } else {
                None
            };
            if let Some(reason) = reason {
                self.close_output();
                self.output_slot.state = LinkState::Error(reason.into());
                self.output_slot.retry_at = now + RETRY_AFTER;
            }
        }
        if self.output.is_some() || now < self.output_slot.retry_at {
            return;
        }
        let Some(name) = best_match(&self.devices.output, &output_names).cloned() else {
            self.output_slot.state = if self.devices.output.is_empty() {
                LinkState::Off
            } else {
                LinkState::Missing
            };
            return;
        };
        let Some(device) = find(&self.outputs, &name).cloned() else {
            return;
        };
        match self.open_output(&device) {
            Ok((opened, garbage)) => {
                self.output_slot.state = LinkState::Running;
                self.output = Some(Output {
                    live: Live {
                        opened,
                        name,
                        watchdog: Watchdog::new(now),
                    },
                    garbage,
                });
                // Mics and monitor attach to the new engine on this same tick.
                for s in &mut self.mics {
                    s.retry_at = now;
                }
                self.monitor.retry_at = now;
            }
            Err(e) => {
                self.output_slot.state = LinkState::Error(e);
                self.output_slot.retry_at = now + RETRY_AFTER;
            }
        }
    }

    fn open_output(
        &mut self,
        device: &cpal::Device,
    ) -> Result<(Opened, Consumer<Garbage>), String> {
        let supported = streams::output_config(device)?;
        let mut last_err = String::new();
        for buffer in buffer_choices(self.devices.buffer_frames) {
            let (engine, link) = Engine::new(self.shared.clone(), supported.sample_rate());
            match streams::open_output_with(device, &supported, buffer, engine) {
                Ok(opened) => {
                    self.port.set(Some(link.cmds));
                    return Ok((opened, link.garbage));
                }
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    fn close_output(&mut self) {
        self.close_mic(0);
        self.close_mic(1);
        self.close_monitor();
        self.port.set(None);
        if let Some(out) = self.output.take() {
            drop(out.live.opened.stream); // stops the callback and frees the engine
            let mut garbage = out.garbage;
            while garbage.pop().is_ok() {}
        }
        self.output_slot.state = LinkState::Missing;
    }

    fn collect_garbage(&mut self) {
        if let Some(out) = &mut self.output {
            while let Ok(g) = out.garbage.pop() {
                drop(g);
            }
        }
    }

    fn out_info(&self) -> Option<(u32, u32)> {
        self.output
            .as_ref()
            .map(|o| (o.live.opened.rate, o.live.opened.period))
    }

    // ---- monitor (headphones) ----

    fn check_monitor(&mut self, now: Instant) {
        let output_names = names(&self.outputs);
        let wanted = self.devices.monitor_enabled && !self.devices.monitor.is_empty();
        if let Some(live) = &mut self.monitor.live {
            let failed = live.opened.health.failed()
                || !output_names.contains(&live.name)
                || live.watchdog.stalled(&live.opened.health, now);
            if failed || !wanted || self.output.is_none() {
                self.close_monitor();
                if failed {
                    self.monitor.state = LinkState::Error("device lost".into());
                    self.monitor.retry_at = now + RETRY_AFTER;
                }
            }
        }
        if !wanted {
            self.monitor.state = LinkState::Off;
            return;
        }
        if self.monitor.live.is_some() || now < self.monitor.retry_at {
            return;
        }
        let Some((out_rate, out_period)) = self.out_info() else {
            self.monitor.state = LinkState::Waiting;
            return;
        };
        let Some(name) = best_match(&self.devices.monitor, &output_names).cloned() else {
            self.monitor.state = LinkState::Missing;
            return;
        };
        let Some(device) = find(&self.outputs, &name).cloned() else {
            return;
        };
        match self.open_monitor(&device, out_rate, out_period) {
            Ok(opened) => {
                self.monitor.state = LinkState::Running;
                self.monitor.live = Some(Live {
                    opened,
                    name,
                    watchdog: Watchdog::new(now),
                });
            }
            Err(e) => {
                self.monitor.state = LinkState::Error(e);
                self.monitor.retry_at = now + RETRY_AFTER;
            }
        }
    }

    fn open_monitor(
        &mut self,
        device: &cpal::Device,
        out_rate: u32,
        out_period: u32,
    ) -> Result<Opened, String> {
        let supported = streams::output_config(device)?;
        let rate = supported.sample_rate();
        let mut last_err = String::new();
        for buffer in buffer_choices(self.devices.buffer_frames) {
            let period = match buffer {
                BufferSize::Fixed(n) => n,
                BufferSize::Default => rate / 100,
            };
            // Engine -> monitor ring: half a second of stereo.
            let (tx, rx) = RingBuffer::new(out_rate as usize);
            let target = target_fill(out_rate, out_period, rate, period);
            let reader = AdaptiveReader::new(rx, 2, out_rate, rate, target)?;
            match streams::open_output_with(device, &supported, buffer, MonitorSource { reader }) {
                Ok(opened) => {
                    if !self.port.send(EngineCmd::AttachMonitor(Some(tx))) {
                        return Err("engine not running".into());
                    }
                    return Ok(opened);
                }
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    fn close_monitor(&mut self) {
        if self.monitor.live.take().is_some() {
            self.port.send(EngineCmd::AttachMonitor(None));
        }
    }

    // ---- mics ----

    fn wanted_mic(&self, slot: usize) -> &str {
        if slot == 0 {
            &self.devices.primary
        } else {
            &self.devices.fallback
        }
    }

    fn check_mic(&mut self, slot: usize, now: Instant) {
        let input_names = names(&self.inputs);
        let matched = best_match(self.wanted_mic(slot), &input_names).cloned();
        if let Some(live) = &mut self.mics[slot].live {
            let reason = if live.opened.health.failed() {
                Some("device error")
            } else if matched.as_deref() != Some(live.name.as_str()) {
                Some("unplugged")
            } else if live.watchdog.stalled(&live.opened.health, now) {
                Some("stopped responding")
            } else {
                None
            };
            if let Some(reason) = reason {
                self.close_mic(slot);
                self.mics[slot].state = LinkState::Error(reason.into());
                self.mics[slot].retry_at = now + RETRY_AFTER;
            }
        }
        if self.mics[slot].live.is_some() {
            return;
        }
        let Some(name) = matched else {
            self.mics[slot].state = if self.wanted_mic(slot).is_empty() {
                LinkState::Off
            } else {
                LinkState::Missing
            };
            return;
        };
        let Some((out_rate, out_period)) = self.out_info() else {
            self.mics[slot].state = LinkState::Waiting;
            return;
        };
        if now < self.mics[slot].retry_at {
            return;
        }
        let Some(device) = find(&self.inputs, &name).cloned() else {
            return;
        };
        match self.open_mic(slot, &device, out_rate, out_period) {
            Ok(opened) => {
                self.mics[slot].state = LinkState::Running;
                self.mics[slot].live = Some(Live {
                    opened,
                    name,
                    watchdog: Watchdog::new(now),
                });
            }
            Err(e) => {
                self.mics[slot].state = LinkState::Error(e);
                self.mics[slot].retry_at = now + RETRY_AFTER;
            }
        }
    }

    fn open_mic(
        &mut self,
        slot: usize,
        device: &cpal::Device,
        out_rate: u32,
        out_period: u32,
    ) -> Result<Opened, String> {
        let mut last_err = String::new();
        for buffer in buffer_choices(self.devices.buffer_frames) {
            match streams::open_input(device, buffer, self.shared.clone(), slot) {
                Ok((opened, rx)) => {
                    let target = target_fill(opened.rate, opened.period, out_rate, out_period);
                    let reader = AdaptiveReader::new(rx, 1, opened.rate, out_rate, target)?;
                    if !self
                        .port
                        .send(EngineCmd::AttachMic(slot, Some(Box::new(reader))))
                    {
                        return Err("engine not running".into());
                    }
                    return Ok(opened);
                }
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    fn close_mic(&mut self, slot: usize) {
        if self.mics[slot].live.take().is_some() {
            self.port.send(EngineCmd::AttachMic(slot, None));
        }
        self.shared.mic_level[slot].set(0.0);
    }

    // ---- which mic ----

    fn select_mic(&mut self, now: Instant) {
        let dt = now.duration_since(self.last_tick).as_secs_f32().min(0.5);
        let running = [self.mics[0].live.is_some(), self.mics[1].live.is_some()];
        let sel = self.selector.update(
            &self.mic,
            MicInputs {
                primary_configured: !self.devices.primary.is_empty(),
                running,
                primary_level_db: to_db(self.shared.mic_level[0].get()),
                dt,
            },
        );
        if running[0] {
            self.mics[0].state = if sel.primary_silent {
                LinkState::Silent
            } else {
                LinkState::Running
            };
        }
        self.reason = sel.reason;
        let new = sel.active;
        if sel.changed {
            self.active = new;
            self.shared.set_active_mic(new);
            if now.duration_since(self.started) > STARTUP_GRACE {
                let _ = self.events.send(Event::Switched {
                    to: new,
                    reason: self.reason.clone(),
                });
            }
        }
    }

    fn latency_ms(&self) -> Option<f32> {
        let slot = self.active.slot()?;
        let mic = self.mics[slot].live.as_ref()?;
        let out = self.output.as_ref()?;
        let ms = |frames: u32, rate: u32| frames as f32 * 1000.0 / rate.max(1) as f32;
        let (o, m) = (&out.live.opened, &mic.opened);
        Some(
            ms(m.period, m.rate)
                + self.shared.mic_fill_ms[slot].get()
                + ms(BLOCK as u32 + 2, o.rate)
                + self.shared.chain_latency_ms.get()
                + ms(o.period, o.rate),
        )
    }

    fn publish(&self) {
        let endpoint = |slot: &Slot| Endpoint {
            device: slot.live.as_ref().map(|l| l.name.clone()),
            state: slot.state.clone(),
            rate: slot.live.as_ref().map_or(0, |l| l.opened.rate),
            period: slot.live.as_ref().map_or(0, |l| l.opened.period),
        };
        let output = Endpoint {
            device: self.output.as_ref().map(|o| o.live.name.clone()),
            state: self.output_slot.state.clone(),
            rate: self.output.as_ref().map_or(0, |o| o.live.opened.rate),
            period: self.output.as_ref().map_or(0, |o| o.live.opened.period),
        };
        let outputs = names(&self.outputs);
        let status = DeviceStatus {
            scanned: self.scanned,
            inputs: names(&self.inputs),
            cable: find_cable_input(&outputs).cloned(),
            outputs,
            default_input: self.default_input.clone(),
            output,
            monitor: endpoint(&self.monitor),
            mics: [endpoint(&self.mics[0]), endpoint(&self.mics[1])],
            active: self.active,
            reason: self.reason.clone(),
            latency_ms: self.latency_ms(),
        };
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = status;
    }
}
