//! Real-time side of the soundboard: a fixed pool of voices playing preloaded buffers.
//! Decoding and resampling happen elsewhere; here we only read memory and add.

use crate::audio::block::{BLOCK, StereoBlock};
use crate::dsp::smooth::{Ramp, Smoother};
use std::sync::Arc;

/// A decoded sound: interleaved stereo at `sample_rate`.
#[derive(Debug)]
pub struct SoundData {
    pub frames: Vec<f32>,
    pub sample_rate: u32,
}

impl SoundData {
    pub fn len_frames(&self) -> usize {
        self.frames.len() / 2
    }
}

pub type SoundId = u64;

pub enum SoundCmd {
    /// Start (or restart) a sound.
    Play {
        id: SoundId,
        data: Arc<SoundData>,
        volume: f32,
    },
    SetVolume(SoundId, f32),
    Stop(SoundId),
    StopAll,
}

/// A buffer the mixer let go of; dropped off the audio thread.
pub type SoundRetired = Arc<SoundData>;

pub const VOICES: usize = 16;
const FADE_IN_S: f32 = 0.004;
const FADE_OUT_S: f32 = 0.012;
const RATE: f32 = 48_000.0; // only for fade lengths; exact rate does not matter there

struct Voice {
    id: SoundId,
    data: Arc<SoundData>,
    pos: f64,
    step: f64,
    volume: f32,
    gain: Smoother,
    fade: Ramp,
    stopping: bool,
}

pub struct SoundMixer {
    voices: [Option<Voice>; VOICES],
    out_rate: f64,
}

/// Up to one retired buffer per voice, returned by value so the caller can hand them off.
pub type Retired = [Option<SoundRetired>; VOICES + 1];

impl Default for SoundMixer {
    fn default() -> Self {
        Self::new()
    }
}

impl SoundMixer {
    pub fn new() -> Self {
        Self::with_rate(48_000)
    }

    pub fn with_rate(out_rate: u32) -> Self {
        Self {
            voices: std::array::from_fn(|_| None),
            out_rate: f64::from(out_rate),
        }
    }

    pub fn set_output_rate(&mut self, rate: u32) {
        self.out_rate = f64::from(rate);
    }

    pub fn playing(&self) -> usize {
        self.voices.iter().filter(|v| v.is_some()).count()
    }

    pub fn is_playing(&self, id: SoundId) -> bool {
        self.voices.iter().flatten().any(|v| v.id == id && !v.stopping)
    }

    /// Ids of the voices playing (not fading out) per slot; 0 = empty slot.
    pub fn ids(&self) -> [SoundId; VOICES] {
        std::array::from_fn(|i| match &self.voices[i] {
            Some(v) if !v.stopping => v.id,
            _ => 0,
        })
    }

    /// Applies a command; may return a buffer that is no longer needed.
    pub fn apply(&mut self, cmd: SoundCmd) -> Option<SoundRetired> {
        match cmd {
            SoundCmd::Play { id, data, volume } => {
                // Fade out an existing instance; the new one starts from the top.
                for v in self.voices.iter_mut().flatten() {
                    if v.id == id {
                        v.stopping = true;
                    }
                }
                let step = f64::from(data.sample_rate) / self.out_rate;
                let voice = Voice {
                    id,
                    data,
                    pos: 0.0,
                    step,
                    volume,
                    gain: Smoother::new(RATE, 0.02, volume),
                    fade: Ramp::new(RATE, FADE_IN_S, 0.0),
                    stopping: false,
                };
                let slot = match self.voices.iter().position(Option::is_none) {
                    Some(i) => i,
                    // All busy: steal the voice that has played longest.
                    None => self
                        .voices
                        .iter()
                        .enumerate()
                        .max_by(|a, b| {
                            let pa = a.1.as_ref().map_or(0.0, |v| v.pos);
                            let pb = b.1.as_ref().map_or(0.0, |v| v.pos);
                            pa.total_cmp(&pb)
                        })
                        .map_or(0, |(i, _)| i),
                };
                self.voices[slot].replace(voice).map(|old| old.data)
            }
            SoundCmd::SetVolume(id, vol) => {
                for v in self.voices.iter_mut().flatten() {
                    if v.id == id {
                        v.volume = vol;
                    }
                }
                None
            }
            SoundCmd::Stop(id) => {
                for v in self.voices.iter_mut().flatten() {
                    if v.id == id {
                        v.stopping = true;
                    }
                }
                None
            }
            SoundCmd::StopAll => {
                for v in self.voices.iter_mut().flatten() {
                    v.stopping = true;
                }
                None
            }
        }
    }

    /// Adds all voices into `out` and returns the buffers of voices that finished.
    pub fn mix(&mut self, out: &mut StereoBlock) -> Retired {
        let mut retired: Retired = std::array::from_fn(|_| None);
        for (slot, entry) in self.voices.iter_mut().enumerate() {
            let Some(v) = entry else { continue };
            let frames = &v.data.frames;
            let len = v.data.len_frames();
            let fade_out = Ramp::new(RATE, FADE_OUT_S, v.fade.value());
            if v.stopping {
                v.fade = fade_out;
            }
            let mut finished = false;
            for i in 0..BLOCK {
                let idx = v.pos as usize;
                if idx + 1 >= len {
                    finished = true;
                    break;
                }
                let frac = (v.pos - idx as f64) as f32;
                let target = if v.stopping { 0.0 } else { 1.0 };
                let g = v.gain.next(v.volume) * v.fade.next(target);
                for c in 0..2 {
                    let a = frames[2 * idx + c];
                    let b = frames[2 * idx + 2 + c];
                    out[2 * i + c] += (a + (b - a) * frac) * g;
                }
                v.pos += v.step;
                if v.stopping && v.fade.value() <= 0.0 {
                    finished = true;
                    break;
                }
            }
            if finished {
                retired[slot] = entry.take().map(|v| v.data);
            }
        }
        retired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, rate: u32) -> Arc<SoundData> {
        Arc::new(SoundData {
            frames: (0..frames).flat_map(|_| [0.5, -0.5]).collect(),
            sample_rate: rate,
        })
    }

    fn render(m: &mut SoundMixer, blocks: usize) -> (Vec<f32>, usize) {
        let mut all = Vec::new();
        let mut retired = 0;
        for _ in 0..blocks {
            let mut out = [0.0; BLOCK * 2];
            retired += m.mix(&mut out).iter().flatten().count();
            all.extend_from_slice(&out);
        }
        (all, retired)
    }

    #[test]
    fn plays_to_the_end_with_fade_in_then_retires() {
        let mut m = SoundMixer::with_rate(48_000);
        assert!(m.apply(SoundCmd::Play { id: 1, data: tone(4800, 48_000), volume: 1.0 }).is_none());
        let (out, retired) = render(&mut m, 100);
        assert!(out[0].abs() < 0.01, "starts faded in");
        assert!((out[2000] - 0.5).abs() < 0.01);
        assert_eq!(retired, 1);
        assert_eq!(m.playing(), 0);
    }

    #[test]
    fn stop_fades_out_instead_of_cutting() {
        let mut m = SoundMixer::with_rate(48_000);
        m.apply(SoundCmd::Play { id: 7, data: tone(48_000, 48_000), volume: 1.0 });
        render(&mut m, 10);
        m.apply(SoundCmd::StopAll);
        let (out, retired) = render(&mut m, 20);
        let left: Vec<f32> = out.chunks_exact(2).map(|f| f[0]).collect();
        let max_step = left.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(max_step < 0.01, "step {max_step}");
        assert_eq!(retired, 1);
    }

    #[test]
    fn sounds_at_other_rates_keep_their_duration() {
        let mut m = SoundMixer::with_rate(48_000);
        // One second at 24 kHz must last about one second at 48 kHz.
        m.apply(SoundCmd::Play { id: 1, data: tone(24_000, 24_000), volume: 1.0 });
        let mut blocks = 0;
        while m.playing() > 0 && blocks < 2000 {
            let mut out = [0.0; BLOCK * 2];
            m.mix(&mut out);
            blocks += 1;
        }
        let secs = (blocks * BLOCK) as f32 / 48_000.0;
        assert!((secs - 1.0).abs() < 0.01, "{secs}");
    }

    #[test]
    fn retrigger_restarts_and_voice_pool_steals_when_full() {
        let mut m = SoundMixer::with_rate(48_000);
        let data = tone(480_000, 48_000);
        m.apply(SoundCmd::Play { id: 1, data: data.clone(), volume: 1.0 });
        m.apply(SoundCmd::Play { id: 1, data: data.clone(), volume: 1.0 });
        assert_eq!(m.playing(), 2, "old instance fades out while the new one starts");
        render(&mut m, 20);
        assert_eq!(m.playing(), 1);
        for id in 0..(VOICES as u64 + 3) {
            m.apply(SoundCmd::Play { id: 100 + id, data: data.clone(), volume: 0.1 });
        }
        assert_eq!(m.playing(), VOICES);
    }
}
