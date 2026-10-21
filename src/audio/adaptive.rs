//! Reading audio that another device clock produces.
//!
//! A mic at "48 kHz" and VB-CABLE at "48 kHz" are driven by different crystals, so one is
//! always a little faster. If we just copied samples across, the ring buffer between them would
//! slowly fill up (growing latency) or run dry (clicks). Instead we resample with rubato's
//! asynchronous resampler and nudge its ratio so the ring stays at a target fill level: a
//! simple proportional controller. The same reader also converts between different nominal
//! rates (44.1 kHz mic -> 48 kHz cable).

use super::block::BLOCK;
use rtrb::Consumer;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Adjustable, Async, FixedAsync, PolynomialDegree, Resampler};

/// Largest ratio correction the controller may apply (1% = 0.17 semitone, inaudible on voice).
const MAX_CORRECTION: f64 = 0.01;
/// Correction per unit of relative fill error.
const GAIN: f64 = 0.0075;
/// Time constant of the fill average.
const AVERAGE_S: f64 = 1.0;

pub struct AdaptiveReader {
    rx: Consumer<f32>,
    channels: usize,
    resampler: Async<f32>,
    scratch: Vec<f32>,
    /// Desired average fill, in source frames.
    target: f64,
    max_target: f64,
    avg_fill: f64,
    avg_alpha: f64,
    primed: bool,
    src_rate: f64,
    underruns: u32,
}

impl AdaptiveReader {
    /// `rx` carries interleaved samples with `channels` per frame at `src_rate`; blocks come out
    /// at `dst_rate`. `target_frames` is the fill to aim for (bigger = safer but more latency).
    pub fn new(
        rx: Consumer<f32>,
        channels: usize,
        src_rate: u32,
        dst_rate: u32,
        target_frames: f64,
    ) -> Result<Self, String> {
        let ratio = f64::from(dst_rate) / f64::from(src_rate);
        let resampler = Async::<f32>::new_poly(
            ratio,
            1.0 + 2.0 * MAX_CORRECTION,
            PolynomialDegree::Cubic,
            BLOCK,
            channels,
            FixedAsync::Output,
        )
        .map_err(|e| format!("resampler: {e}"))?;
        let scratch = vec![0.0; resampler.input_frames_max() * channels];
        let blocks_per_s = f64::from(dst_rate) / BLOCK as f64;
        let target = target_frames.max(BLOCK as f64);
        Ok(Self {
            rx,
            channels,
            resampler,
            scratch,
            target,
            max_target: (target * 4.0).max(f64::from(src_rate) * 0.1),
            avg_fill: 0.0,
            avg_alpha: 1.0 / (AVERAGE_S * blocks_per_s),
            primed: false,
            src_rate: f64::from(src_rate),
            underruns: 0,
        })
    }

    /// Writes exactly [`BLOCK`] interleaved frames into `out`. Never blocks or allocates; on
    /// underrun it outputs silence and waits until the ring has refilled to the target.
    pub fn read(&mut self, out: &mut [f32]) {
        let ch = self.channels;
        debug_assert_eq!(out.len(), BLOCK * ch);
        let mut fill = self.rx.slots() / ch;

        if !self.primed {
            if (fill as f64) < self.target {
                out.fill(0.0);
                return;
            }
            self.primed = true;
            self.avg_fill = fill as f64;
        }

        // Way too full (the output stalled, or we just started): skip ahead instead of
        // carrying seconds of latency around.
        if fill as f64 > self.target * 4.0 + 2.0 * BLOCK as f64 {
            let drop = fill - self.target as usize;
            if let Ok(chunk) = self.rx.read_chunk(drop * ch) {
                chunk.commit_all();
            }
            fill -= drop;
            self.avg_fill = fill as f64;
        }

        self.avg_fill += (fill as f64 - self.avg_fill) * self.avg_alpha;
        let error = (self.avg_fill - self.target) / self.target;
        let correction = (error * GAIN).clamp(-MAX_CORRECTION, MAX_CORRECTION);
        // Ratio = out/in: a lower ratio consumes more input per block, draining the ring.
        let _ = self
            .resampler
            .set_resample_ratio_relative(1.0 - correction, true);

        let need = self.resampler.input_frames_next();
        if fill < need {
            // Underrun: be a bit more careful from now on.
            self.underruns = self.underruns.wrapping_add(1);
            self.primed = false;
            self.target = (self.target * 1.25).min(self.max_target);
            out.fill(0.0);
            return;
        }
        let Ok(chunk) = self.rx.read_chunk(need * ch) else {
            out.fill(0.0);
            return;
        };
        let (a, b) = chunk.as_slices();
        self.scratch[..a.len()].copy_from_slice(a);
        self.scratch[a.len()..a.len() + b.len()].copy_from_slice(b);
        chunk.commit_all();

        let input = InterleavedSlice::new(&self.scratch[..need * ch], ch, need);
        let output = InterleavedSlice::new_mut(out, ch, BLOCK);
        let ok = match (input, output) {
            (Ok(input), Ok(mut output)) => self
                .resampler
                .process_into_buffer(&input, &mut output, None)
                .is_ok(),
            _ => false,
        };
        if !ok {
            out.fill(0.0);
        }
    }

    /// Average buffering in ms (part of the end-to-end latency).
    pub fn fill_ms(&self) -> f32 {
        (self.avg_fill / self.src_rate * 1000.0) as f32
    }

    #[cfg(test)]
    pub fn underruns(&self) -> u32 {
        self.underruns
    }
}

/// A sensible starting fill: half a source period of jitter, one destination period that is
/// consumed in one go, one block and a 2 ms margin, all in source frames.
pub fn target_fill(src_rate: u32, src_period: u32, dst_rate: u32, dst_period: u32) -> f64 {
    let to_src = f64::from(src_rate) / f64::from(dst_rate);
    0.5 * f64::from(src_period)
        + f64::from(dst_period) * to_src
        + BLOCK as f64 * to_src
        + 0.002 * f64::from(src_rate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::RingBuffer;

    /// Simulates a producer callback of `src_period` frames at `src_rate * drift` against a
    /// consumer pulling `dst_period` frames at `dst_rate`, for `seconds` of audio.
    fn simulate(src_rate: u32, drift: f64, dst_rate: u32, seconds: f64) -> (AdaptiveReader, f64) {
        let (src_period, dst_period) = (src_rate / 100, dst_rate / 100);
        let (mut tx, rx) = RingBuffer::new(src_rate as usize);
        let target = target_fill(src_rate, src_period, dst_rate, dst_period);
        let mut reader = AdaptiveReader::new(rx, 1, src_rate, dst_rate, target).unwrap();

        let src_dt = f64::from(src_period) / (f64::from(src_rate) * drift);
        let dst_dt = f64::from(dst_period) / f64::from(dst_rate);
        let (mut t_src, mut t_dst) = (0.0, 0.0);
        let mut phase = 0.0f64;
        let mut out = [0f32; BLOCK];
        let mut produced_blocks = 0usize;
        let mut pending = 0usize; // destination frames still owed this period
        while t_dst < seconds {
            if t_src <= t_dst {
                for _ in 0..src_period {
                    let _ = tx.push((phase * 0.01).sin() as f32);
                    phase += 1.0;
                }
                t_src += src_dt;
            } else {
                pending += dst_period as usize;
                while pending >= BLOCK {
                    reader.read(&mut out);
                    pending -= BLOCK;
                    produced_blocks += 1;
                }
                t_dst += dst_dt;
            }
        }
        let _ = produced_blocks;
        let fill = reader.avg_fill;
        (reader, fill)
    }

    #[test]
    fn locks_onto_a_faster_source_without_growing_latency() {
        let (reader, fill) = simulate(48_000, 1.002, 48_000, 60.0);
        assert!(
            fill < reader.target * 1.5,
            "fill {fill} target {}",
            reader.target
        );
        assert!(reader.underruns() <= 1, "underruns {}", reader.underruns());
    }

    #[test]
    fn locks_onto_a_slower_source_without_underrunning() {
        let (reader, fill) = simulate(48_000, 0.998, 48_000, 60.0);
        assert!(fill > reader.target * 0.5, "fill {fill}");
        assert!(reader.underruns() <= 2, "underruns {}", reader.underruns());
    }

    #[test]
    fn converts_44k1_to_48k() {
        let (reader, fill) = simulate(44_100, 1.0, 48_000, 30.0);
        assert!(
            (fill - reader.target).abs() < reader.target * 0.5,
            "fill {fill}"
        );
        assert!(reader.underruns() <= 1);
    }

    #[test]
    fn a_tone_passes_through_at_the_right_frequency() {
        // 1 kHz at 44.1 kHz in, count zero crossings at 48 kHz out.
        let (mut tx, rx) = RingBuffer::new(20_000);
        let mut reader = AdaptiveReader::new(rx, 1, 44_100, 48_000, 800.0).unwrap();
        let mut out = vec![0f32; 0];
        let mut block = [0f32; BLOCK];
        let (mut i, mut pending) = (0u64, 0usize);
        for _ in 0..200 {
            // 10 ms periods on both sides.
            for _ in 0..441 {
                let t = i as f64 / 44_100.0;
                tx.push((2.0 * std::f64::consts::PI * 1000.0 * t).sin() as f32)
                    .unwrap();
                i += 1;
            }
            pending += 480;
            while pending >= BLOCK {
                reader.read(&mut block);
                out.extend_from_slice(&block);
                pending -= BLOCK;
            }
        }
        let tail = &out[4800..]; // skip start-up
        let crossings = tail
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count() as f64;
        let seconds = tail.len() as f64 / 48_000.0;
        let freq = crossings / seconds;
        assert!((freq - 1000.0).abs() < 15.0, "measured {freq} Hz");
    }
}
