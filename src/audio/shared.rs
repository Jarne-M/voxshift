//! State shared between the GUI, the device manager and the audio callbacks.
//! Everything here is a plain atomic, so the audio threads never wait for a lock.

use crate::dsp::cue::CueKind;
use crate::dsp::params::ParamStore;
use crate::melody::store::MelodyStore;
use crate::soundboard::mixer::VOICES;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, AtomicU64, Ordering::Relaxed};

/// An `f32` stored in an `AtomicU32` (bit-cast). Relaxed ordering is enough: each value
/// is independent and a reader seeing it one block late is harmless.
#[derive(Debug, Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub const fn zero() -> Self {
        Self(AtomicU32::new(0))
    }

    pub fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Relaxed))
    }

    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Relaxed);
    }
}

/// Which microphone feeds the voice chain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActiveMic {
    #[default]
    None = 0,
    Primary = 1,
    Fallback = 2,
}

impl ActiveMic {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Primary,
            2 => Self::Fallback,
            _ => Self::None,
        }
    }

    /// Slot index (0 = primary, 1 = fallback).
    pub fn slot(self) -> Option<usize> {
        match self {
            Self::None => None,
            Self::Primary => Some(0),
            Self::Fallback => Some(1),
        }
    }
}

pub struct Shared {
    /// Every effect parameter (see `dsp::params`).
    pub params: ParamStore,
    /// Master toggle: false = clean passthrough.
    pub voice_on: AtomicBool,
    /// Mutes the mic (fades out like a mic switch); the soundboard keeps playing.
    pub mic_muted: AtomicBool,
    /// Beep in the headphones (monitor only) when the voice changer or the monitor switches.
    pub cue_enabled: AtomicBool,
    pub cue_volume: AtomicF32,
    /// The beep to play ([`CueKind::code`], 0 = nothing). Taken by the engine.
    cue_request: AtomicU8,
    pub overlay_enabled: AtomicBool,
    active_mic: AtomicU8,
    /// Per-mic level (linear peak with ~300 ms release), written by the input callbacks.
    pub mic_level: [AtomicF32; 2],
    /// Average ring-buffer fill per mic in ms (part of the latency), written by the engine.
    pub mic_fill_ms: [AtomicF32; 2],
    /// Level of the mic signal entering the voice chain (after the mic crossfade).
    pub input_level: AtomicF32,
    pub output_level: AtomicF32,
    /// Bumped every time an output sample exceeded full scale.
    pub clip_count: AtomicU32,
    /// Extra latency added by the voice chain right now (pitch shifter), in ms.
    pub chain_latency_ms: AtomicF32,
    pub monitor_volume: AtomicF32,
    /// "Hear myself": your voice on the monitor output. The checkbox, the tray menu and the
    /// hotkey all flip this one value.
    pub monitor_voice: AtomicBool,
    pub monitor_sounds: AtomicBool,
    /// Master soundboard volume (0..1), on top of each clip's own volume. Applies to the
    /// virtual mic and the headphones alike.
    pub sound_volume: AtomicF32,
    /// The clip previewed in the headphones only (0 = none) and how far it is (0..1).
    pub listen_id: AtomicU64,
    pub listen_progress: AtomicF32,
    /// Bumped to stop every soundboard clip (they fade out); acted on by the engine.
    pub stop_sounds: AtomicU32,
    /// Panic / safe mode: the engine sends the clean mic (no effects, no Melody Voice), plays
    /// no clips and, if `safe_mutes_monitor`, mutes the monitor. Nothing else is changed, so
    /// clearing it restores exactly what was before. Never saved: VoxShift starts normal.
    pub safe_mode: AtomicBool,
    pub safe_mutes_monitor: AtomicBool,
    /// Number of soundboard voices playing.
    pub sounds_playing: AtomicU32,
    /// Sound id per mixer voice (0 = free), so the GUI can highlight playing sounds.
    pub playing_ids: [AtomicU64; VOICES],
    /// How far each mixer voice is through its clip (0..1), for the progress bar on a pad.
    pub playing_progress: [AtomicF32; VOICES],
    /// Melody Voice: your pitch and the note you are tuned to (MIDI numbers, NaN = none).
    pub melody_detected: AtomicF32,
    pub melody_target: AtomicF32,
    /// The melody being sung (written by the window, copied by the audio thread).
    pub melody: MelodyStore,
    /// Bumped to send Melody Voice back to the first note / on to the next note.
    pub melody_restart: AtomicU32,
    pub melody_next: AtomicU32,
    /// Current note index (-1 = before the first) and playhead in beats.
    pub melody_note: AtomicI32,
    pub melody_beat: AtomicF32,
    /// Reached the end of the melody (loop off).
    pub melody_finished: AtomicBool,
    /// "Play melody" in the headphones: set by the window, cleared by the engine at the end.
    pub melody_preview: AtomicBool,
    pub melody_preview_beat: AtomicF32,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            params: ParamStore::default(),
            voice_on: AtomicBool::new(true),
            mic_muted: AtomicBool::new(false),
            cue_enabled: AtomicBool::new(true),
            cue_volume: AtomicF32::new(0.5),
            cue_request: AtomicU8::new(0),
            overlay_enabled: AtomicBool::new(false),
            active_mic: AtomicU8::new(0),
            mic_level: [AtomicF32::zero(), AtomicF32::zero()],
            mic_fill_ms: [AtomicF32::zero(), AtomicF32::zero()],
            input_level: AtomicF32::zero(),
            output_level: AtomicF32::zero(),
            clip_count: AtomicU32::new(0),
            chain_latency_ms: AtomicF32::zero(),
            monitor_volume: AtomicF32::new(0.8),
            monitor_voice: AtomicBool::new(true),
            monitor_sounds: AtomicBool::new(true),
            sound_volume: AtomicF32::new(1.0),
            listen_id: AtomicU64::new(0),
            listen_progress: AtomicF32::zero(),
            stop_sounds: AtomicU32::new(0),
            safe_mode: AtomicBool::new(false),
            safe_mutes_monitor: AtomicBool::new(false),
            sounds_playing: AtomicU32::new(0),
            playing_ids: std::array::from_fn(|_| AtomicU64::new(0)),
            playing_progress: std::array::from_fn(|_| AtomicF32::zero()),
            melody_detected: AtomicF32::new(f32::NAN),
            melody_target: AtomicF32::new(f32::NAN),
            melody: MelodyStore::default(),
            melody_restart: AtomicU32::new(0),
            melody_next: AtomicU32::new(0),
            melody_note: AtomicI32::new(-1),
            melody_beat: AtomicF32::zero(),
            melody_finished: AtomicBool::new(false),
            melody_preview: AtomicBool::new(false),
            melody_preview_beat: AtomicF32::zero(),
        }
    }
}

impl Shared {
    /// Asks the engine to play a beep on the monitor output.
    pub fn request_cue(&self, kind: CueKind) {
        self.cue_request.store(kind.code(), Relaxed);
    }

    /// Takes a pending beep request (engine side).
    pub fn take_cue(&self) -> Option<CueKind> {
        CueKind::from_code(self.cue_request.swap(0, Relaxed))
    }

    pub fn is_playing(&self, id: u64) -> bool {
        id != 0 && self.playing_ids.iter().any(|a| a.load(Relaxed) == id)
    }

    /// How far clip `id` is through playing (0..1), if it is playing.
    pub fn progress_of(&self, id: u64) -> Option<f32> {
        if id == 0 {
            return None;
        }
        let slot = self
            .playing_ids
            .iter()
            .position(|a| a.load(Relaxed) == id)?;
        Some(self.playing_progress[slot].get())
    }

    /// The ids of the clips playing right now, in mixer order.
    pub fn playing(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .playing_ids
            .iter()
            .map(|a| a.load(Relaxed))
            .filter(|&id| id != 0)
            .collect();
        ids.dedup();
        ids
    }

    pub fn active_mic(&self) -> ActiveMic {
        ActiveMic::from_u8(self.active_mic.load(Relaxed))
    }

    pub fn set_active_mic(&self, m: ActiveMic) {
        self.active_mic.store(m as u8, Relaxed);
    }
}

/// Converts a linear amplitude to dBFS, with silence clamped to -120 dB.
pub fn to_db(lin: f32) -> f32 {
    if lin <= 1e-6 {
        -120.0
    } else {
        20.0 * lin.log10()
    }
}

pub fn from_db(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Peak meter with exponential release, as used by every callback that publishes a level.
#[derive(Clone, Copy, Debug)]
pub struct PeakMeter {
    level: f32,
    release_per_frame: f32,
}

impl PeakMeter {
    /// `release_s`: time for the level to fall by ~63%.
    pub fn new(sample_rate: f32, release_s: f32) -> Self {
        Self {
            level: 0.0,
            release_per_frame: (-1.0 / (release_s * sample_rate)).exp(),
        }
    }

    /// Feeds a block peak covering `frames` frames and returns the new level.
    pub fn update(&mut self, peak: f32, frames: usize) -> f32 {
        let decayed = self.level * self.release_per_frame.powi(frames as i32);
        self.level = peak.max(decayed);
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_conversions_round_trip() {
        assert!((to_db(1.0)).abs() < 1e-6);
        assert!((to_db(from_db(-20.0)) + 20.0).abs() < 1e-4);
        assert_eq!(to_db(0.0), -120.0);
    }

    #[test]
    fn peak_meter_holds_peaks_and_releases() {
        let mut m = PeakMeter::new(48_000.0, 0.3);
        assert_eq!(m.update(0.5, 64), 0.5);
        let after = m.update(0.0, 48_000 * 3 / 10);
        assert!((after - 0.5 * (-1f32).exp()).abs() < 0.01, "{after}");
    }
}
