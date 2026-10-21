//! State shared between the GUI, the device manager and the audio callbacks.
//! Everything here is a plain atomic, so the audio threads never wait for a lock.

use crate::dsp::params::ParamStore;
use crate::soundboard::mixer::VOICES;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering::Relaxed};

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
    pub monitor_voice: AtomicBool,
    pub monitor_sounds: AtomicBool,
    /// Number of soundboard voices playing.
    pub sounds_playing: AtomicU32,
    /// Sound id per mixer voice (0 = free), so the GUI can highlight playing sounds.
    pub playing_ids: [AtomicU64; VOICES],
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            params: ParamStore::default(),
            voice_on: AtomicBool::new(true),
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
            sounds_playing: AtomicU32::new(0),
            playing_ids: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

impl Shared {
    pub fn is_playing(&self, id: u64) -> bool {
        id != 0 && self.playing_ids.iter().any(|a| a.load(Relaxed) == id)
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
