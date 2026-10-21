//! Parameter smoothing. Jumping a gain or mix value from one sample to the next makes a click;
//! gliding it over a few milliseconds does not.

/// One-pole low-pass on a control value: moves ~63% of the way to the target per time constant.
#[derive(Clone, Copy, Debug)]
pub struct Smoother {
    value: f32,
    coeff: f32,
}

impl Smoother {
    pub fn new(sample_rate: f32, time_s: f32, initial: f32) -> Self {
        Self {
            value: initial,
            coeff: coeff(sample_rate, time_s),
        }
    }

    /// Advances one sample toward `target`.
    #[inline]
    pub fn next(&mut self, target: f32) -> f32 {
        self.value += (target - self.value) * self.coeff;
        self.value
    }

    /// Advances `n` samples at once (for values that are only applied per block).
    pub fn next_n(&mut self, target: f32, n: usize) -> f32 {
        let k = 1.0 - (1.0 - self.coeff).powi(n as i32);
        self.value += (target - self.value) * k;
        self.value
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn reset(&mut self, v: f32) {
        self.value = v;
    }

    /// True once the value has (practically) arrived.
    pub fn settled(&self, target: f32) -> bool {
        (self.value - target).abs() < 1e-4
    }
}

fn coeff(sample_rate: f32, time_s: f32) -> f32 {
    1.0 - (-1.0 / (time_s.max(1e-5) * sample_rate)).exp()
}

/// Linear ramp to a target over a fixed time (used for crossfades, which must end exactly).
#[derive(Clone, Copy, Debug)]
pub struct Ramp {
    value: f32,
    step: f32,
}

impl Ramp {
    pub fn new(sample_rate: f32, time_s: f32, initial: f32) -> Self {
        Self {
            value: initial,
            step: 1.0 / (time_s * sample_rate).max(1.0),
        }
    }

    #[inline]
    pub fn next(&mut self, target: f32) -> f32 {
        if self.value < target {
            self.value = (self.value + self.step).min(target);
        } else if self.value > target {
            self.value = (self.value - self.step).max(target);
        }
        // Float steps rarely add up exactly; snap the last bit.
        if (self.value - target).abs() < self.step * 0.5 {
            self.value = target;
        }
        self.value
    }

    pub fn value(&self) -> f32 {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoother_reaches_target_without_overshoot() {
        let mut s = Smoother::new(48_000.0, 0.01, 0.0);
        let mut prev = 0.0;
        for _ in 0..4800 {
            let v = s.next(1.0);
            assert!(v >= prev && v <= 1.0);
            prev = v;
        }
        assert!(s.settled(1.0));
    }

    #[test]
    fn next_n_matches_n_single_steps() {
        let mut a = Smoother::new(48_000.0, 0.02, 0.0);
        let mut b = a;
        for _ in 0..64 {
            a.next(0.5);
        }
        b.next_n(0.5, 64);
        assert!((a.value() - b.value()).abs() < 1e-5);
    }

    #[test]
    fn ramp_takes_exactly_the_given_time() {
        let mut r = Ramp::new(1000.0, 0.05, 0.0); // 50 steps
        for _ in 0..49 {
            assert!(r.next(1.0) < 1.0);
        }
        assert_eq!(r.next(1.0), 1.0);
    }
}
