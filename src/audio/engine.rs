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
use crate::dsp::smooth::Ramp;
use crate::dsp::{Block, VoiceProcessor};
use crate::soundboard::mixer::{SoundCmd, SoundMixer, SoundRetired};
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::sync::Arc;
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
    /// Last Melody Voice restart request acted on.
    melody_restart_seen: u32,
}

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
        if let Some(on) = self.shared.take_cue()
            && self.monitor.is_some()
        {
            self.cue.start(on);
        }
        let mut cue = [0.0; BLOCK];
        if self.cue.active() {
            let vol = self.shared.cue_volume.get() * 0.5;
            for c in cue.iter_mut() {
                *c = self.cue.next() * vol;
            }
        }
        if let Some(tx) = &mut self.monitor {
            let vol = self.shared.monitor_volume.get();
            let gv = if self.shared.monitor_voice.load(Relaxed) {
                vol
            } else {
                0.0
            };
            let gs = if self.shared.monitor_sounds.load(Relaxed) {
                vol
            } else {
                0.0
            };
            if let Ok(chunk) = tx.write_chunk_uninit(BLOCK * 2) {
                let it = (0..BLOCK * 2)
                    .map(|j| (voice[j / 2] * gv + sfx[j] * gs + cue[j / 2]).clamp(-1.0, 1.0));
                chunk.fill_from_iter(it);
            }
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
        shared.request_cue(true);
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
        let (speech, _) = speech(3, 0.22, 0.13);
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
