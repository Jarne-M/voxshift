//! "Mic check": record five seconds of exactly what the virtual mic sends (your voice with
//! its effects, plus any clip), then play it back in your headphones.
//!
//! The recording comes from the engine's tap, a lock-free ring the audio thread copies its
//! output into (it drops blocks that don't fit, it never waits). The window empties the ring
//! every frame. Playback goes through the preview mixer, so it never reaches the virtual mic.

use crate::audio::device_manager::EnginePort;
use crate::audio::engine::EngineCmd;
use crate::audio::shared::Shared;
use crate::soundboard::mixer::{SoundCmd, SoundData, SoundId};
use rtrb::{Consumer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

/// The recording's id in the preview mixer (no clip ever gets it).
pub const ID: SoundId = u64::MAX;
pub const SECONDS: f64 = 5.0;
/// Room in the ring: the window may miss a few frames (it is drawn ~30 times a second).
const RING_SECONDS: f64 = 2.0;
/// The recording ends by the clock too, in case the engine stops sending.
const GRACE_S: f64 = 1.0;

/// Samples (stereo, interleaved) in a full recording.
fn wanted_samples(rate: u32) -> usize {
    (f64::from(rate) * SECONDS) as usize * 2
}

#[derive(Default)]
pub enum MicCheck {
    #[default]
    Idle,
    Recording {
        rx: Consumer<f32>,
        samples: Vec<f32>,
        rate: u32,
        started: f64,
    },
    Playing {
        started: f64,
        length: f64,
        /// The engine has reported it as playing at least once.
        seen: bool,
    },
}

/// What a step of the check came to.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Recording done, now playing back.
    PlayingBack,
    Finished,
    /// Nothing was recorded (no engine running).
    Failed,
}

impl MicCheck {
    pub fn busy(&self) -> bool {
        !matches!(self, Self::Idle)
    }

    #[cfg(test)]
    pub fn recording(&self) -> bool {
        matches!(self, Self::Recording { .. })
    }

    /// Starts recording. `rate`: the virtual mic's sample rate. False if no engine runs.
    pub fn start(&mut self, port: &EnginePort, rate: u32, now: f64) -> bool {
        self.cancel(port);
        let rate = if rate == 0 { 48_000 } else { rate };
        let (tx, rx) = RingBuffer::new((f64::from(rate) * RING_SECONDS) as usize * 2);
        if !port.send(EngineCmd::AttachTap(Some(tx))) {
            return false;
        }
        *self = Self::Recording {
            rx,
            samples: Vec::with_capacity(wanted_samples(rate)),
            rate,
            started: now,
        };
        true
    }

    /// Stops whatever it is doing.
    pub fn cancel(&mut self, port: &EnginePort) {
        match self {
            Self::Recording { .. } => {
                port.send(EngineCmd::AttachTap(None));
            }
            Self::Playing { .. } => {
                port.send(EngineCmd::Listen(SoundCmd::Stop(ID)));
            }
            Self::Idle => {}
        }
        *self = Self::Idle;
    }

    /// How far along it is (0..1) for the current phase.
    pub fn progress(&self, now: f64) -> f32 {
        match self {
            Self::Idle => 0.0,
            Self::Recording { samples, rate, .. } => {
                (samples.len() as f64 / wanted_samples(*rate) as f64).min(1.0) as f32
            }
            Self::Playing {
                started, length, ..
            } => ((now - started) / length.max(0.1)).clamp(0.0, 1.0) as f32,
        }
    }

    /// Call every frame: collects the recording, starts the playback and notices its end.
    pub fn tick(&mut self, port: &EnginePort, shared: &Shared, now: f64) -> Option<Outcome> {
        match self {
            Self::Idle => None,
            Self::Recording {
                rx,
                samples,
                rate,
                started,
            } => {
                let wanted = wanted_samples(*rate);
                let n = rx.slots().min(wanted - samples.len());
                if let Ok(chunk) = rx.read_chunk(n) {
                    samples.extend(chunk);
                }
                let full = samples.len() >= wanted;
                if !full && now - *started < SECONDS + GRACE_S {
                    return None;
                }
                port.send(EngineCmd::AttachTap(None));
                if samples.len() < (*rate as usize / 10) * 2 {
                    *self = Self::Idle;
                    return Some(Outcome::Failed);
                }
                let data = Arc::new(SoundData {
                    frames: std::mem::take(samples),
                    sample_rate: *rate,
                });
                let length = data.len_frames() as f64 / f64::from(*rate);
                port.send(EngineCmd::Listen(SoundCmd::StopAll));
                port.send(EngineCmd::Listen(SoundCmd::Play {
                    id: ID,
                    data,
                    volume: 1.0,
                    looped: false,
                }));
                *self = Self::Playing {
                    started: now,
                    length,
                    seen: false,
                };
                Some(Outcome::PlayingBack)
            }
            Self::Playing {
                started,
                length,
                seen,
            } => {
                let playing = shared.listen_id.load(Relaxed) == ID;
                *seen |= playing;
                // Over when the engine is done with it (or, if it never started it, after
                // its length).
                let over = (*seen && !playing) || now - *started > *length + GRACE_S;
                if over {
                    *self = Self::Idle;
                    Some(Outcome::Finished)
                } else {
                    None
                }
            }
        }
    }
}

/// The "Mic check" button; while busy it shows the phase and its time, and a click stops it.
/// Returns true when clicked.
pub fn button(ui: &mut egui::Ui, check: &MicCheck, now: f64) -> bool {
    use super::icons;
    use super::theme::palette;
    use egui::RichText;
    let p = palette(ui);
    let (text, color, tip) = match check {
        MicCheck::Idle => (
            format!("{}  Mic check", icons::RECORD),
            p.text_muted,
            "Records 5 seconds of exactly what others hear (your voice with its effects, \
             and any clip), then plays it back in your headphones",
        ),
        MicCheck::Recording { .. } => {
            // (Less a hair, so a float that lands just above 3 still says 3.)
            let left = (SECONDS * f64::from(1.0 - check.progress(now)) - 1e-3).ceil();
            (
                format!("{}  Recording\u{2026} {left:.0} s", icons::RECORD),
                p.danger,
                "Talk now. Click to stop.",
            )
        }
        MicCheck::Playing { .. } => (
            format!("{}  Playing back\u{2026}", icons::HEADPHONES),
            p.accent,
            "This is what others heard. Click to stop.",
        ),
    };
    let r = ui
        .button(RichText::new(text).color(color))
        .on_hover_text(tip);
    #[cfg(test)]
    super::widgets::remember(ui.ctx(), "voice", "mic_check", r.rect);
    r.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::block::{BLOCK, BlockSource};
    use crate::audio::engine::Engine;

    #[test]
    fn records_what_the_virtual_mic_sends_and_plays_it_in_the_headphones() {
        let shared = Arc::new(Shared::default());
        let (mut engine, link) = Engine::new(shared.clone(), 48_000);
        let port = EnginePort::default();
        port.set(Some(link.cmds));
        let (mon_tx, mut mon_rx) = RingBuffer::new(48_000 * 8);
        port.send(EngineCmd::AttachMonitor(Some(mon_tx)));
        shared.monitor_voice.store(false, Relaxed);
        shared.monitor_volume.set(1.0);

        let mut check = MicCheck::default();
        assert!(check.start(&port, 48_000, 0.0));
        assert!(check.recording());
        // The engine sends a steady clip... (a clip stands in for the voice: no mic here).
        port.send(EngineCmd::Sound(SoundCmd::Play {
            id: 1,
            data: Arc::new(SoundData {
                frames: vec![0.25; 48_000 * 20],
                sample_rate: 48_000,
            }),
            volume: 1.0,
            looped: false,
        }));
        let mut out = [0.0; BLOCK * 2];
        let mut t = 0.0;
        let blocks_per_frame = 48_000 / 30 / BLOCK + 1;
        let mut outcome = None;
        while outcome.is_none() {
            for _ in 0..blocks_per_frame {
                engine.render(&mut out);
            }
            t += 1.0 / 30.0;
            outcome = check.tick(&port, &shared, t);
            assert!(t < 10.0, "never finished recording");
        }
        assert_eq!(outcome, Some(Outcome::PlayingBack));
        assert!((SECONDS - 0.2..SECONDS + 0.5).contains(&t), "{t}");
        // Stop the clip itself, so what the headphones get now is the recording alone.
        port.send(EngineCmd::Sound(SoundCmd::StopAll));
        while mon_rx.pop().is_ok() {}
        for _ in 0..200 {
            engine.render(&mut out);
        }
        assert_eq!(shared.listen_id.load(Relaxed), ID);
        let mut heard = Vec::new();
        while let Ok(x) = mon_rx.pop() {
            heard.push(x);
        }
        let level = heard[heard.len() - 2];
        assert!(
            (level - 0.25).abs() < 1e-3,
            "the recording plays back: {level}"
        );
        assert!(out.iter().all(|x| x.abs() < 1e-6), "not on the virtual mic");
        assert!(check.tick(&port, &shared, t + 0.1).is_none());
        // Five seconds later it is over.
        for _ in 0..(48_000 * 5 / BLOCK) {
            engine.render(&mut out);
        }
        assert_eq!(check.tick(&port, &shared, t + 5.5), Some(Outcome::Finished));
        assert!(!check.busy());
    }

    #[test]
    fn without_an_engine_it_does_not_start() {
        let port = EnginePort::default();
        let mut check = MicCheck::default();
        assert!(!check.start(&port, 48_000, 0.0));
        assert!(!check.busy());
    }
}
