//! The real-time engine, run block by block from the output (virtual mic) callback.
//!
//! Per block: take one block from each mic reader, crossfade between them according to the
//! active mic, run the voice chain, mix in the soundboard, clip-protect, meter, and feed the
//! monitor. New readers arrive through a command queue; replaced ones leave through a
//! "garbage" queue so they are freed on the device manager thread, never here.

use super::adaptive::AdaptiveReader;
use super::block::{BLOCK, BlockSource, StereoBlock};
use super::shared::{PeakMeter, Shared};
use crate::dsp::chain::VoiceChain;
use crate::dsp::cue::Cue;
use crate::dsp::params::P;
use crate::dsp::preview::Preview;
use crate::dsp::smooth::Ramp;
use crate::dsp::{Block, VoiceProcessor};
use crate::soundboard::mixer::{SoundCmd, SoundMixer, SoundRetired};
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;

/// Mic crossfade time.
pub const MIC_FADE_S: f32 = 0.05;
pub const CMD_QUEUE: usize = 256;

pub enum EngineCmd {
    /// Replace the reader of mic slot 0 (primary) or 1 (fallback).
    AttachMic(usize, Option<Box<AdaptiveReader>>),
    AttachMonitor(Option<Producer<f32>>),
    Sound(SoundCmd),
}

/// Things the audio thread is done with, to be dropped elsewhere. The payloads are never read:
/// they only travel here so their memory is freed on the device manager thread.
#[allow(dead_code)]
pub enum Garbage {
    Mic(Box<AdaptiveReader>),
    Monitor(Producer<f32>),
    Sound(SoundRetired),
}

pub struct Engine {
    cmds: Consumer<EngineCmd>,
    garbage: Producer<Garbage>,
    shared: Arc<Shared>,
    mics: [Option<Box<AdaptiveReader>>; 2],
    mic_buf: [Block; 2],
    mic_gain: [Ramp; 2],
    chain: VoiceChain,
    sounds: SoundMixer,
    monitor: Option<Producer<f32>>,
    cue: Cue,
    in_meter: PeakMeter,
    out_meter: PeakMeter,
    sample_rate: f32,
    /// Last Melody Voice restart / next-note requests acted on.
    melody_restart_seen: u32,
    melody_next_seen: u32,
    preview: Preview,
    preview_was_playing: bool,
    /// Last "stop all sounds" request acted on.
    stop_sounds_seen: u32,
    /// Monitor gains for your voice and the soundboard (0..1, ramped).
    monitor_gain: [Ramp; 2],
}

/// How long the monitor takes to fade your voice (or the soundboard) in or out.
pub const MONITOR_RAMP_S: f32 = 0.02;

/// The manager's ends of the engine queues.
pub struct EngineLink {
    pub cmds: Producer<EngineCmd>,
    pub garbage: Consumer<Garbage>,
}

impl Engine {
    pub fn new(shared: Arc<Shared>, sample_rate: u32) -> (Self, EngineLink) {
        let (cmd_tx, cmd_rx) = RingBuffer::new(CMD_QUEUE);
        let (garbage_tx, garbage_rx) = RingBuffer::new(CMD_QUEUE);
        let sr = sample_rate as f32;
        let melody_restart_seen = shared.melody_restart.load(Relaxed);
        let melody_next_seen = shared.melody_next.load(Relaxed);
        let stop_sounds_seen = shared.stop_sounds.load(Relaxed);
        let gain =
            |b: &AtomicBool| Ramp::new(sr, MONITOR_RAMP_S, if b.load(Relaxed) { 1.0 } else { 0.0 });
        let monitor_gain = [gain(&shared.monitor_voice), gain(&shared.monitor_sounds)];
        let engine = Self {
            cmds: cmd_rx,
            garbage: garbage_tx,
            shared,
            mics: [None, None],
            mic_buf: [[0.0; BLOCK]; 2],
            mic_gain: [Ramp::new(sr, MIC_FADE_S, 0.0); 2],
            chain: VoiceChain::new(sr),
            sounds: SoundMixer::with_rate(sample_rate),
            monitor: None,
            cue: Cue::new(sr),
            in_meter: PeakMeter::new(sr, 0.3),
            out_meter: PeakMeter::new(sr, 0.3),
            sample_rate: sr,
            melody_restart_seen,
            melody_next_seen,
            preview: Preview::new(sr),
            preview_was_playing: false,
            stop_sounds_seen,
            monitor_gain,
        };
        let link = EngineLink {
            cmds: cmd_tx,
            garbage: garbage_rx,
        };
        (engine, link)
    }

    fn retire(&mut self, g: Garbage) {
        if let Err(PushError::Full(g)) = self.garbage.push(g) {
            // Never free memory on the audio thread; leaking is the lesser evil (and the
            // manager drains this queue every 50 ms, so it does not happen in practice).
            std::mem::forget(g);
        }
    }

    fn apply(&mut self, cmd: EngineCmd) {
        match cmd {
            EngineCmd::AttachMic(slot, reader) => {
                if slot < 2
                    && let Some(old) = std::mem::replace(&mut self.mics[slot], reader)
                {
                    self.retire(Garbage::Mic(old));
                }
            }
            EngineCmd::AttachMonitor(tx) => {
                if let Some(old) = std::mem::replace(&mut self.monitor, tx) {
                    self.retire(Garbage::Monitor(old));
                }
            }
            EngineCmd::Sound(cmd) => {
                if let Some(done) = self.sounds.apply(cmd) {
                    self.retire(Garbage::Sound(done));
                }
            }
        }
    }

    /// Mixes the two mic slots according to the active mic, with a linear crossfade.
    fn mic_block(&mut self) -> Block {
        for slot in 0..2 {
            match &mut self.mics[slot] {
                Some(reader) => {
                    reader.read(&mut self.mic_buf[slot]);
                    self.shared.mic_fill_ms[slot].set(reader.fill_ms());
                }
                None => self.mic_buf[slot].fill(0.0),
            }
        }
        // Muting just aims both mic gains at zero: the 50 ms ramp makes it click-free.
        let active = if self.shared.mic_muted.load(Relaxed) {
            None
        } else {
            self.shared.active_mic().slot()
        };
        let targets = [active == Some(0), active == Some(1)].map(|on| if on { 1.0 } else { 0.0 });
        let mut voice = [0.0; BLOCK];
        for (i, v) in voice.iter_mut().enumerate() {
            let g0 = self.mic_gain[0].next(targets[0]);
            let g1 = self.mic_gain[1].next(targets[1]);
            *v = g0 * self.mic_buf[0][i] + g1 * self.mic_buf[1][i];
        }
        voice
    }
}

impl BlockSource for Engine {
    fn render(&mut self, out: &mut StereoBlock) {
        while let Ok(cmd) = self.cmds.pop() {
            self.apply(cmd);
        }

        let mut voice = self.mic_block();
        let in_peak = peak(&voice);
        self.shared
            .input_level
            .set(self.in_meter.update(in_peak, BLOCK));

        let params = self.shared.params.snapshot();
        let voice_on = self.shared.voice_on.load(Relaxed);
        let melody = self.chain.melody();
        melody.sync(&self.shared.melody);
        let restart = self.shared.melody_restart.load(Relaxed);
        if restart != self.melody_restart_seen {
            self.melody_restart_seen = restart;
            melody.restart();
        }
        let next = self.shared.melody_next.load(Relaxed);
        for _ in 0..next.wrapping_sub(self.melody_next_seen).min(16) {
            melody.next_note();
        }
        self.melody_next_seen = next;
        self.chain.run(&mut voice, &params, voice_on);
        let melody = self.chain.melody().status();
        self.shared.melody_detected.set(melody.detected);
        self.shared.melody_target.set(melody.target);
        let pos = melody.position;
        self.shared
            .melody_note
            .store(pos.note.map_or(-1, |n| n as i32), Relaxed);
        self.shared.melody_beat.set(pos.beat);
        self.shared.melody_finished.store(pos.finished, Relaxed);
        let latency = if voice_on {
            self.chain.latency_samples()
        } else {
            0
        };
        self.shared
            .chain_latency_ms
            .set(latency as f32 * 1000.0 / self.sample_rate);

        // "Stop all sounds" from the hotkey: the same fading StopAll as the button.
        let stop = self.shared.stop_sounds.load(Relaxed);
        if stop != self.stop_sounds_seen {
            self.stop_sounds_seen = stop;
            if let Some(done) = self.sounds.apply(SoundCmd::StopAll) {
                self.retire(Garbage::Sound(done));
            }
        }
        let mut sfx = [0.0; BLOCK * 2];
        for done in self.sounds.mix(&mut sfx).into_iter().flatten() {
            self.retire(Garbage::Sound(done));
        }
        self.shared
            .sounds_playing
            .store(self.sounds.playing() as u32, Relaxed);
        for (slot, id) in self.shared.playing_ids.iter().zip(self.sounds.ids()) {
            slot.store(id, Relaxed);
        }

        let mut out_peak = 0.0f32;
        let mut clipped = false;
        for i in 0..BLOCK {
            for c in 0..2 {
                let x = voice[i] + sfx[2 * i + c];
                out_peak = out_peak.max(x.abs());
                clipped |= x.abs() > 1.0;
                out[2 * i + c] = x.clamp(-1.0, 1.0);
            }
        }
        if clipped {
            self.shared.clip_count.fetch_add(1, Relaxed);
        }
        self.shared
            .output_level
            .set(self.out_meter.update(out_peak, BLOCK));

        // Switch beeps go to the monitor (headphones) only, never into the virtual mic.
        if let Some(kind) = self.shared.take_cue()
            && self.monitor.is_some()
        {
            self.cue.start(kind);
        }
        let mut cue = [0.0; BLOCK];
        if self.cue.active() {
            let vol = self.shared.cue_volume.get() * 0.5;
            for c in cue.iter_mut() {
                *c = self.cue.next() * vol;
            }
        }
        // "Play melody": the tune in the headphones, also never into the virtual mic.
        self.preview.sync(&self.shared.melody);
        let wanted = self.shared.melody_preview.load(Relaxed);
        if wanted && !self.preview.playing() {
            if !self.preview_was_playing {
                self.preview.start();
            }
            if !self.preview.playing() {
                // It reached the end (or there is nothing to play): tell the window.
                self.shared.melody_preview.store(false, Relaxed);
            }
        } else if !wanted && self.preview.playing() {
            self.preview.stop();
        }
        self.preview_was_playing = self.preview.playing();
        if self.preview.active() {
            let mut tune = [0.0; BLOCK];
            self.preview.render(
                &mut tune,
                params.get(P::MelodyBpm),
                params.get(P::MelodyTranspose).round(),
            );
            let vol = self.shared.monitor_volume.get();
            for (c, t) in cue.iter_mut().zip(tune) {
                *c += t * vol;
            }
        }
        self.shared.melody_preview_beat.set(self.preview.beat());
        // "Hear myself" and "hear the soundboard" ramp over 20 ms instead of switching, so
        // turning the monitor on never blasts in at full level (and off never clicks). The
        // ramps run even without a monitor device, so they are always where they should be.
        let on = |b: bool| if b { 1.0 } else { 0.0 };
        let (tv, ts) = (
            on(self.shared.monitor_voice.load(Relaxed)),
            on(self.shared.monitor_sounds.load(Relaxed)),
        );
        let vol = self.shared.monitor_volume.get();
        let mut gv = [0.0; BLOCK];
        let mut gs = [0.0; BLOCK];
        for (v, s) in gv.iter_mut().zip(gs.iter_mut()) {
            *v = self.monitor_gain[0].next(tv) * vol;
            *s = self.monitor_gain[1].next(ts) * vol;
        }
        if let Some(tx) = &mut self.monitor
            && let Ok(chunk) = tx.write_chunk_uninit(BLOCK * 2)
        {
            let it = (0..BLOCK * 2).map(|j| {
                let i = j / 2;
                (voice[i] * gv[i] + sfx[j] * gs[i] + cue[i]).clamp(-1.0, 1.0)
            });
            chunk.fill_from_iter(it);
        }
    }
}

fn peak(b: &[f32]) -> f32 {
    b.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

/// Plays the monitor ring on the headphones.
pub struct MonitorSource {
    pub reader: AdaptiveReader,
}

impl BlockSource for MonitorSource {
    fn render(&mut self, out: &mut StereoBlock) {
        self.reader.read(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::shared::ActiveMic;
    use crate::dsp::cue::CueKind;

    fn mic(level: f32) -> (Producer<f32>, Box<AdaptiveReader>) {
        let (tx, rx) = RingBuffer::new(48_000);
        let reader = AdaptiveReader::new(rx, 1, 48_000, 48_000, 256.0).unwrap();
        let _ = level;
        (tx, Box::new(reader))
    }

    fn feed(tx: &mut Producer<f32>, v: f32, n: usize) {
        for _ in 0..n {
            let _ = tx.push(v);
        }
    }

    /// Runs `blocks` blocks while feeding both mics a constant, returns the left channel.
    fn run(
        engine: &mut Engine,
        mics: &mut [Producer<f32>; 2],
        values: [f32; 2],
        blocks: usize,
    ) -> Vec<f32> {
        let mut out = [0.0; BLOCK * 2];
        let mut left = Vec::new();
        for _ in 0..blocks {
            feed(&mut mics[0], values[0], BLOCK);
            feed(&mut mics[1], values[1], BLOCK);
            engine.render(&mut out);
            left.extend(out.as_chunks::<2>().0.iter().map(|f| f[0]));
        }
        left
    }

    #[test]
    fn crossfades_between_mics_without_jumps() {
        let shared = Arc::new(Shared::default());
        let (mut engine, mut link) = Engine::new(shared.clone(), 48_000);
        let (tx0, r0) = mic(0.5);
        let (tx1, r1) = mic(0.2);
        link.cmds
            .push(EngineCmd::AttachMic(0, Some(r0)))
            .ok()
            .unwrap();
        link.cmds
            .push(EngineCmd::AttachMic(1, Some(r1)))
            .ok()
            .unwrap();
        let mut txs = [tx0, tx1];
        // Pre-fill both rings so the readers are primed.
        feed(&mut txs[0], 0.5, 512);
        feed(&mut txs[1], 0.2, 512);

        shared.set_active_mic(ActiveMic::Primary);
        let a = run(&mut engine, &mut txs, [0.5, 0.2], 100);
        assert!(
            (a.last().unwrap() - 0.5).abs() < 1e-3,
            "primary: {}",
            a.last().unwrap()
        );

        shared.set_active_mic(ActiveMic::Fallback);
        let b = run(&mut engine, &mut txs, [0.5, 0.2], 100);
        assert!((b.last().unwrap() - 0.2).abs() < 1e-3);
        let max_step = b
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        // 0.3 difference spread over 50 ms (2400 samples).
        assert!(max_step < 0.001, "click: step {max_step}");

        shared.set_active_mic(ActiveMic::None);
        let c = run(&mut engine, &mut txs, [0.5, 0.2], 100);
        assert!(c.last().unwrap().abs() < 1e-6);
    }

    #[test]
    fn mute_fades_the_mic_out_and_back_without_clicks() {
        let shared = Arc::new(Shared::default());
        let (mut engine, mut link) = Engine::new(shared.clone(), 48_000);
        let (tx0, r0) = mic(0.5);
        let (tx1, _r1) = mic(0.0);
        link.cmds
            .push(EngineCmd::AttachMic(0, Some(r0)))
            .ok()
            .unwrap();
        let mut txs = [tx0, tx1];
        feed(&mut txs[0], 0.5, 512);
        shared.set_active_mic(ActiveMic::Primary);
        run(&mut engine, &mut txs, [0.5, 0.0], 100);

        shared.mic_muted.store(true, Relaxed);
        let muted = run(&mut engine, &mut txs, [0.5, 0.0], 100);
        assert!(muted.last().unwrap().abs() < 1e-6, "silent while muted");
        let step = muted
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(step < 0.001, "mute must fade, step {step}");

        shared.mic_muted.store(false, Relaxed);
        let back = run(&mut engine, &mut txs, [0.5, 0.0], 100);
        assert!((back.last().unwrap() - 0.5).abs() < 1e-3);
    }

    #[test]
    fn switch_beep_goes_to_the_monitor_only() {
        let shared = Arc::new(Shared::default());
        let (mut engine, mut link) = Engine::new(shared.clone(), 48_000);
        let (mon_tx, mut mon_rx) = RingBuffer::new(48_000);
        link.cmds
            .push(EngineCmd::AttachMonitor(Some(mon_tx)))
            .ok()
            .unwrap();
        shared.request_cue(crate::dsp::cue::CueKind::VoiceOn);
        let mut out = [0.0; BLOCK * 2];
        let mut out_peak = 0.0f32;
        for _ in 0..150 {
            engine.render(&mut out);
            out_peak = out.iter().fold(out_peak, |m, x| m.max(x.abs()));
        }
        let mut mon_peak = 0.0f32;
        while let Ok(s) = mon_rx.pop() {
            mon_peak = mon_peak.max(s.abs());
        }
        assert_eq!(out_peak, 0.0, "nothing in the virtual mic");
        assert!(mon_peak > 0.05, "beep in the headphones: {mon_peak}");
        assert_eq!(shared.take_cue(), None, "request consumed");
    }

    #[test]
    fn play_melody_goes_to_the_monitor_only_and_ends_by_itself() {
        let shared = Arc::new(Shared::default());
        shared.melody.publish(
            &crate::melody::library::get("Major arpeggio (C)")
                .unwrap()
                .notes,
        );
        shared.params.set(crate::dsp::params::P::MelodyBpm, 240.0); // 9 beats: 2.25 s
        let (mut engine, mut link) = Engine::new(shared.clone(), 48_000);
        let (mon_tx, mut mon_rx) = RingBuffer::new(48_000 * 4);
        link.cmds
            .push(EngineCmd::AttachMonitor(Some(mon_tx)))
            .ok()
            .unwrap();
        shared.melody_preview.store(true, Relaxed);
        let mut out = [0.0; BLOCK * 2];
        let (mut out_peak, mut moved) = (0.0f32, false);
        for n in 0..(48_000 * 3 / BLOCK) {
            engine.render(&mut out);
            out_peak = out.iter().fold(out_peak, |m, x| m.max(x.abs()));
            moved |= n > 100 && shared.melody_preview_beat.get() > 2.0;
        }
        let mut mon_peak = 0.0f32;
        while let Ok(s) = mon_rx.pop() {
            mon_peak = mon_peak.max(s.abs());
        }
        assert_eq!(out_peak, 0.0, "nothing in the virtual mic");
        assert!(mon_peak > 0.1, "the tune in the headphones: {mon_peak}");
        assert!(moved, "the playhead moves");
        assert!(!shared.melody_preview.load(Relaxed), "cleared at the end");

        // Stopping halfway also works, and an empty melody clears the request at once.
        shared.melody_preview.store(true, Relaxed);
        for _ in 0..50 {
            engine.render(&mut out);
        }
        shared.melody_preview.store(false, Relaxed);
        for _ in 0..300 {
            engine.render(&mut out); // 0.4 s: the 30 ms release has died away
        }
        assert!(!engine.preview.active());
        shared.melody.publish(&[]);
        shared.melody_preview.store(true, Relaxed);
        engine.render(&mut out);
        assert!(!shared.melody_preview.load(Relaxed));
    }

    /// An engine with a mic on slot 0 (primary active) and a monitor ring.
    fn rig() -> (Arc<Shared>, Engine, Producer<f32>, rtrb::Consumer<f32>) {
        let shared = Arc::new(Shared::default());
        shared.set_active_mic(ActiveMic::Primary);
        let (mut engine, mut link) = Engine::new(shared.clone(), 48_000);
        let (mut tx, reader) = mic(0.0);
        link.cmds
            .push(EngineCmd::AttachMic(0, Some(reader)))
            .ok()
            .unwrap();
        let (mon_tx, mon_rx) = RingBuffer::new(48_000 * 4);
        link.cmds
            .push(EngineCmd::AttachMonitor(Some(mon_tx)))
            .ok()
            .unwrap();
        feed(&mut tx, 0.0, 512);
        let mut out = [0.0; BLOCK * 2];
        engine.render(&mut out); // take the commands
        (shared, engine, tx, mon_rx)
    }

    /// Renders `blocks` blocks with `voice(sample index)` on the mic; returns (virtual mic
    /// left channel, monitor left channel).
    fn talk(
        engine: &mut Engine,
        tx: &mut Producer<f32>,
        mon: &mut rtrb::Consumer<f32>,
        start: usize,
        blocks: usize,
        voice: impl Fn(usize) -> f32,
    ) -> (Vec<f32>, Vec<f32>) {
        let (mut out_l, mut mon_l) = (Vec::new(), Vec::new());
        let mut out = [0.0; BLOCK * 2];
        for b in 0..blocks {
            for i in 0..BLOCK {
                let _ = tx.push(voice(start + b * BLOCK + i));
            }
            engine.render(&mut out);
            out_l.extend(out.as_chunks::<2>().0.iter().map(|f| f[0]));
            while let Ok(l) = mon.pop() {
                mon_l.push(l);
                let _ = mon.pop(); // right
            }
        }
        (out_l, mon_l)
    }

    fn sine(i: usize) -> f32 {
        0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48_000.0).sin()
    }

    #[test]
    fn stop_all_sounds_fades_the_clips_out_and_leaves_the_voice_alone() {
        use crate::soundboard::mixer::{SoundCmd, SoundData};
        // Two identical engines hear the same voice; one also plays a long clip (a steady
        // 0.4) and gets "stop all sounds" halfway. Their difference is exactly the clip.
        let (shared_a, mut a, mut tx_a, mut mon_a) = rig();
        let (_shared_b, mut b, mut tx_b, mut mon_b) = rig();
        let clip = Arc::new(SoundData {
            frames: vec![0.4; 48_000 * 10 * 2],
            sample_rate: 48_000,
        });
        a.apply(EngineCmd::Sound(SoundCmd::Play {
            id: 5,
            data: clip,
            volume: 1.0,
        }));
        let (va1, _) = talk(&mut a, &mut tx_a, &mut mon_a, 0, 200, sine);
        let (vb1, _) = talk(&mut b, &mut tx_b, &mut mon_b, 0, 200, sine);
        assert!(shared_a.is_playing(5));
        let before = va1.last().unwrap() - vb1.last().unwrap();
        assert!((before - 0.4).abs() < 1e-4, "the clip plays: {before}");

        shared_a.stop_sounds.fetch_add(1, Relaxed); // what the hotkey does
        let (va2, _) = talk(&mut a, &mut tx_a, &mut mon_a, 200 * BLOCK, 100, sine);
        let (vb2, _) = talk(&mut b, &mut tx_b, &mut mon_b, 200 * BLOCK, 100, sine);
        let clip_part: Vec<f32> = va2.iter().zip(&vb2).map(|(x, y)| x - y).collect();
        // Fades: no jump bigger than a 12 ms ramp of 0.4 allows...
        let step = clip_part
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(step < 0.002, "a fade, not a cut: step {step}");
        // ...is gone within 20 ms (one block of slack for when the request is seen)...
        let gone = clip_part.iter().position(|d| d.abs() < 1e-6).unwrap();
        let ms = gone as f32 / 48.0;
        assert!(
            ms > 5.0 && ms <= 20.0 + BLOCK as f32 / 48.0,
            "faded out in {ms} ms"
        );
        // ...and after it the output is exactly the voice, as if no clip had ever played.
        assert!(
            clip_part[gone..].iter().all(|d| *d == 0.0),
            "voice untouched"
        );
        assert!(!shared_a.is_playing(5) && shared_a.sounds_playing.load(Relaxed) == 0);
        assert!(shared_a.voice_on.load(Relaxed) && shared_a.monitor_voice.load(Relaxed));
    }

    #[test]
    fn hear_myself_ramps_on_the_monitor_only_and_never_touches_the_virtual_mic() {
        let (shared, mut e, mut tx, mut mon) = rig();
        let vol = shared.monitor_volume.get();
        let dc = |_| 0.5;
        let (out, m) = talk(&mut e, &mut tx, &mut mon, 0, 150, dc);
        assert!((out.last().unwrap() - 0.5).abs() < 1e-4);
        assert!(
            (m.last().unwrap() - 0.5 * vol).abs() < 1e-4,
            "hearing myself"
        );

        let ramp = 0.02 * 48_000.0;
        let full = 0.5 * vol;
        let max_step = |v: &[f32]| {
            v.windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0, f32::max)
        };
        for (on, name) in [(false, "off"), (true, "on")] {
            shared.monitor_voice.store(on, Relaxed); // what the hotkey/checkbox/tray do
            let (out, m) = talk(&mut e, &mut tx, &mut mon, 0, 40, dc);
            assert!(
                out.iter().all(|x| (x - 0.5).abs() < 1e-4),
                "{name}: virtual mic untouched"
            );
            // A 20 ms ramp, not a jump (a full-level step would be 0.4).
            assert!(
                max_step(&m) < full / ramp * 1.5,
                "{name}: step {}",
                max_step(&m)
            );
            let end = if on { full } else { 0.0 };
            let at = m.iter().position(|x| (x - end).abs() < 1e-6).unwrap() as f32 / 48.0;
            assert!(at > 15.0 && at < 25.0, "{name}: ramp took {at} ms");
        }

        // The monitor beeps land on the monitor only.
        shared.request_cue(CueKind::MonitorOn);
        let (out, m) = talk(&mut e, &mut tx, &mut mon, 0, 100, dc);
        assert!(
            out.iter().all(|x| (x - 0.5).abs() < 1e-4),
            "no beep in the virtual mic"
        );
        let beep = m.iter().map(|x| (x - full).abs()).fold(0.0f32, f32::max);
        assert!(beep > 0.05, "beep in the headphones: {beep}");
    }

    #[test]
    fn replaced_readers_are_handed_back_not_dropped() {
        let shared = Arc::new(Shared::default());
        let (mut engine, mut link) = Engine::new(shared, 48_000);
        let (_tx0, r0) = mic(0.0);
        let (_tx1, r1) = mic(0.0);
        link.cmds
            .push(EngineCmd::AttachMic(0, Some(r0)))
            .ok()
            .unwrap();
        let mut out = [0.0; BLOCK * 2];
        engine.render(&mut out);
        link.cmds
            .push(EngineCmd::AttachMic(0, Some(r1)))
            .ok()
            .unwrap();
        link.cmds.push(EngineCmd::AttachMic(0, None)).ok().unwrap();
        engine.render(&mut out);
        let mut returned = 0;
        while let Ok(g) = link.garbage.pop() {
            assert!(matches!(g, Garbage::Mic(_)));
            returned += 1;
        }
        assert_eq!(returned, 2);
    }

    /// Melody Voice on and hard-tuning to C major pentatonic.
    fn melody_on(shared: &Shared) {
        use crate::dsp::params::P;
        shared.params.set(P::MelodyOn, 1.0);
        shared.params.set(P::MelodyMode, 3.0);
        shared.params.set(P::MelodyScale, 2.0);
        shared.params.set(P::MelodyRetune, 0.0);
    }

    #[test]
    fn soundboard_clips_never_go_through_melody_voice() {
        use crate::dsp::melody::tests::measured_midi;
        use crate::soundboard::mixer::{SoundCmd, SoundData};
        let shared = Arc::new(Shared::default());
        melody_on(&shared);
        let (mut engine, mut link) = Engine::new(shared.clone(), 48_000);
        // A 3 s clip of a buzzy tone at 150 Hz (D3 + 0.37 st): Melody Voice would pull a
        // voice like that to D3 (the pentatonic note nearest it).
        let mut phase = 0.0f32;
        let frames: Vec<f32> = (0..48_000 * 3)
            .flat_map(|_| {
                phase = (phase + 150.0 / 48_000.0).fract();
                let x = 0.2 * (2.0 * phase - 1.0);
                [x, x]
            })
            .collect();
        let data = Arc::new(SoundData {
            frames,
            sample_rate: 48_000,
        });
        link.cmds
            .push(EngineCmd::Sound(SoundCmd::Play {
                id: 1,
                data,
                volume: 1.0,
            }))
            .ok()
            .unwrap();
        let mut out = [0.0; BLOCK * 2];
        let mut left = Vec::new();
        for _ in 0..1500 {
            engine.render(&mut out);
            left.extend(out.as_chunks::<2>().0.iter().map(|f| f[0]));
        }
        let got = measured_midi(&left, 0.3).unwrap();
        assert!((got - 50.37).abs() < 0.1, "the clip came out at {got:.2}");
        assert!(
            shared.melody_detected.get().is_nan(),
            "Melody Voice never heard it"
        );
    }

    #[test]
    fn the_melody_and_restart_reach_the_audio_thread_and_survive_a_new_engine() {
        use crate::dsp::melody::tests::speech;
        use crate::dsp::params::P;
        use crate::melody::library;
        let shared = Arc::new(Shared::default());
        shared.params.set(P::MelodyOn, 1.0);
        shared
            .melody
            .publish(&library::get("Ode to Joy").unwrap().notes);
        shared.set_active_mic(ActiveMic::Primary);
        let (speech, _) = speech(3, 0.22, 0.13, 1.0);
        let talk = |engine: &mut Engine, tx: &mut Producer<f32>| {
            let mut out = [0.0; BLOCK * 2];
            for chunk in speech.chunks(BLOCK) {
                for &x in chunk {
                    let _ = tx.push(x);
                }
                engine.render(&mut out);
            }
        };
        let (mut engine, mut link) = Engine::new(shared.clone(), 48_000);
        let (mut tx, reader) = mic(0.0);
        link.cmds
            .push(EngineCmd::AttachMic(0, Some(reader)))
            .ok()
            .unwrap();
        feed(&mut tx, 0.0, 512);
        talk(&mut engine, &mut tx);
        assert_eq!(
            shared.melody_note.load(Relaxed),
            2,
            "three syllables, three notes"
        );
        assert!(shared.melody_target.get().is_nan(), "silent at the end");

        shared.melody_restart.fetch_add(1, Relaxed);
        let mut out = [0.0; BLOCK * 2];
        engine.render(&mut out);
        assert_eq!(shared.melody_note.load(Relaxed), -1, "back to the top");

        // Manual mode: the "next note" requests move it, one each.
        shared.params.set(P::MelodyMode, 2.0);
        talk(&mut engine, &mut tx);
        assert_eq!(
            shared.melody_note.load(Relaxed),
            0,
            "talking does not move it"
        );
        shared.melody_next.fetch_add(2, Relaxed);
        talk(&mut engine, &mut tx);
        assert_eq!(shared.melody_note.load(Relaxed), 2);
        shared.params.set(P::MelodyMode, 0.0);
        shared.melody_restart.fetch_add(1, Relaxed);
        engine.render(&mut out);

        // A new output device means a new engine: it still has the melody.
        let (mut engine, mut link) = Engine::new(shared.clone(), 48_000);
        let (mut tx, reader) = mic(0.0);
        link.cmds
            .push(EngineCmd::AttachMic(0, Some(reader)))
            .ok()
            .unwrap();
        feed(&mut tx, 0.0, 512);
        talk(&mut engine, &mut tx);
        assert_eq!(shared.melody_note.load(Relaxed), 2);
    }

    #[test]
    fn output_is_clamped_and_clips_are_counted() {
        let shared = Arc::new(Shared::default());
        shared.params.set(crate::dsp::params::P::InputGain, 24.0);
        let (mut engine, mut link) = Engine::new(shared.clone(), 48_000);
        let (tx0, r0) = mic(0.0);
        link.cmds
            .push(EngineCmd::AttachMic(0, Some(r0)))
            .ok()
            .unwrap();
        let (dummy, _) = mic(0.0);
        let mut txs = [tx0, dummy];
        feed(&mut txs[0], 0.9, 512);
        shared.set_active_mic(ActiveMic::Primary);
        let out = run(&mut engine, &mut txs, [0.9, 0.0], 50);
        assert!(out.iter().all(|x| x.abs() <= 1.0));
        assert!(shared.clip_count.load(Relaxed) > 0);
    }
}
