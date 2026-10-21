//! Fixed-size block processing. Devices call back with whatever buffer size they like (often
//! 441 or 480 frames); everything inside the app works in blocks of [`BLOCK`] stereo frames.

use cpal::{FromSample, Sample};

pub const BLOCK: usize = 64;

/// Stereo interleaved block.
pub type StereoBlock = [f32; BLOCK * 2];

/// Something that renders audio one block at a time (the engine, the monitor).
pub trait BlockSource: Send {
    fn render(&mut self, out: &mut StereoBlock);
}

/// Serves device buffers of any length and channel count from a [`BlockSource`].
pub struct BlockAdapter<S> {
    pub source: S,
    buf: StereoBlock,
    pos: usize,
}

impl<S: BlockSource> BlockAdapter<S> {
    pub fn new(source: S) -> Self {
        Self {
            source,
            buf: [0.0; BLOCK * 2],
            pos: BLOCK,
        }
    }

    /// Fills an interleaved device buffer. Mono devices get L+R, extra channels get silence.
    pub fn fill<T: Sample + FromSample<f32>>(&mut self, data: &mut [T], channels: usize) {
        let channels = channels.max(1);
        for frame in data.chunks_exact_mut(channels) {
            if self.pos == BLOCK {
                self.source.render(&mut self.buf);
                self.pos = 0;
            }
            let (l, r) = (self.buf[2 * self.pos], self.buf[2 * self.pos + 1]);
            self.pos += 1;
            if channels == 1 {
                frame[0] = T::from_sample(0.5 * (l + r));
            } else {
                frame[0] = T::from_sample(l);
                frame[1] = T::from_sample(r);
                for s in &mut frame[2..] {
                    *s = T::EQUILIBRIUM;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Counter(f32);
    impl BlockSource for Counter {
        fn render(&mut self, out: &mut StereoBlock) {
            for f in out.chunks_exact_mut(2) {
                f[0] = self.0;
                f[1] = -self.0;
                self.0 += 1.0;
            }
        }
    }

    #[test]
    fn odd_buffer_sizes_get_a_continuous_signal() {
        let mut a = BlockAdapter::new(Counter(0.0));
        let mut all = Vec::new();
        for len in [441, 7, 1000, 64] {
            let mut buf = vec![0f32; len * 2];
            a.fill(&mut buf, 2);
            all.extend(buf.chunks_exact(2).map(|f| f[0]));
        }
        for (i, v) in all.iter().enumerate() {
            assert_eq!(*v, i as f32);
        }
    }

    #[test]
    fn mono_and_multichannel_layouts() {
        let mut a = BlockAdapter::new(Counter(2.0));
        let mut mono = [1f32; 3];
        a.fill(&mut mono, 1);
        assert_eq!(mono, [0.0; 3], "L and R cancel");
        let mut quad = [9i16; 8];
        a.fill(&mut quad, 4);
        assert_eq!(&quad[2..4], &[0, 0]);
        assert!(quad[0] > 0 && quad[1] < 0);
    }
}
