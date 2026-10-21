//! A circular delay line with fractional (linearly interpolated) reads, shared by echo and chorus.

pub struct DelayLine {
    buf: Vec<f32>,
    mask: usize,
    write: usize,
}

impl DelayLine {
    /// Holds at least `max_samples` of history.
    pub fn new(max_samples: usize) -> Self {
        let len = (max_samples + 2).next_power_of_two();
        Self {
            buf: vec![0.0; len],
            mask: len - 1,
            write: 0,
        }
    }

    pub fn max_delay(&self) -> f32 {
        (self.buf.len() - 2) as f32
    }

    /// Value `delay` samples ago (>= 1; 1 = the most recent push), read before the next `push`.
    #[inline]
    pub fn read(&self, delay: f32) -> f32 {
        let delay = delay.clamp(1.0, self.max_delay());
        let d = delay.floor();
        let frac = delay - d;
        let i = self.write.wrapping_sub(d as usize - 1) & self.mask;
        let j = i.wrapping_sub(1) & self.mask;
        self.buf[i] + (self.buf[j] - self.buf[i]) * frac
    }

    #[inline]
    pub fn push(&mut self, x: f32) {
        self.write = (self.write + 1) & self.mask;
        self.buf[self.write] = x;
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_back_integer_and_fractional_delays() {
        let mut d = DelayLine::new(100);
        for i in 0..50 {
            d.push(i as f32);
        }
        assert_eq!(d.read(1.0), 49.0);
        assert_eq!(d.read(10.0), 40.0);
        assert!((d.read(10.5) - 39.5).abs() < 1e-6);
    }
}
