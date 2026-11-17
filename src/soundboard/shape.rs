//! Trim and fades per clip: where it starts and ends, and how it fades in and out.
//!
//! Applied to a copy of the decoded clip when it loads or when a setting changes (never on the
//! audio thread); the file itself is never changed. A clip without trim or fades shares the
//! decoded buffer as it is.

use super::mixer::SoundData;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Saved per clip. All in seconds; all zero = the clip as recorded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Trim {
    /// Skipped at the start.
    pub start: f32,
    /// Cut off at the end.
    pub end: f32,
    pub fade_in: f32,
    pub fade_out: f32,
}

/// What is always left of a clip, however much is trimmed.
pub const MIN_LENGTH_S: f32 = 0.05;
/// The longest fade the edit panel offers.
pub const MAX_FADE_S: f32 = 5.0;

impl Trim {
    pub fn is_none(&self) -> bool {
        *self == Trim::default()
    }

    /// The same settings, made to fit a clip `length` seconds long: the trims leave at least
    /// [`MIN_LENGTH_S`], and the fades fit in what is left.
    pub fn fitted(&self, length: f32) -> Trim {
        let clean = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
        let room = (length - MIN_LENGTH_S).max(0.0);
        let start = clean(self.start).min(room);
        let end = clean(self.end).min(room - start);
        let left = (length - start - end).max(0.0);
        let fade_in = clean(self.fade_in).min(MAX_FADE_S).min(left);
        let fade_out = clean(self.fade_out).min(MAX_FADE_S).min(left - fade_in);
        Trim {
            start,
            end,
            fade_in,
            fade_out,
        }
    }
}

/// The clip with `trim` applied (shared as it is when there is nothing to do).
pub fn apply(raw: &Arc<SoundData>, trim: &Trim) -> Arc<SoundData> {
    if trim.is_none() {
        return raw.clone();
    }
    let rate = raw.sample_rate as f32;
    let len = raw.len_frames();
    let t = trim.fitted(len as f32 / rate);
    let first = ((t.start * rate) as usize).min(len);
    let last = len.saturating_sub((t.end * rate) as usize).max(first);
    let mut frames = raw.frames[first * 2..last * 2].to_vec();
    let n = last - first;
    let fade_in = ((t.fade_in * rate) as usize).min(n);
    let fade_out = ((t.fade_out * rate) as usize).min(n);
    for (i, pair) in frames.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let mut g = 1.0f32;
        if i < fade_in {
            g *= i as f32 / fade_in as f32;
        }
        let from_end = n - 1 - i;
        if from_end < fade_out {
            g *= from_end as f32 / fade_out as f32;
        }
        if g < 1.0 {
            pair[0] *= g;
            pair[1] *= g;
        }
    }
    Arc::new(SoundData {
        frames,
        sample_rate: raw.sample_rate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steady(seconds: f32) -> Arc<SoundData> {
        Arc::new(SoundData {
            frames: vec![0.5; (1000.0 * seconds) as usize * 2],
            sample_rate: 1000,
        })
    }

    #[test]
    fn nothing_to_do_shares_the_buffer() {
        let raw = steady(1.0);
        assert!(Arc::ptr_eq(&apply(&raw, &Trim::default()), &raw));
    }

    #[test]
    fn trims_both_ends_and_fades_linearly() {
        let raw = steady(2.0);
        let t = Trim {
            start: 0.5,
            end: 0.25,
            fade_in: 0.1,
            fade_out: 0.2,
        };
        let out = apply(&raw, &t);
        assert_eq!(out.len_frames(), 1250);
        let left: Vec<f32> = out.frames.as_chunks::<2>().0.iter().map(|f| f[0]).collect();
        assert_eq!(left[0], 0.0, "fades in from silence");
        assert!((left[50] - 0.25).abs() < 0.01, "halfway in: {}", left[50]);
        assert_eq!(left[600], 0.5, "untouched in the middle");
        assert!((left[1249 - 100] - 0.25).abs() < 0.01, "halfway out");
        assert_eq!(left[1249], 0.0, "ends in silence");
    }

    #[test]
    fn settings_that_do_not_fit_are_fitted() {
        // Trimming more than the clip leaves a sliver; the fades share what is left.
        let t = Trim {
            start: 5.0,
            end: 5.0,
            fade_in: 9.0,
            fade_out: f32::NAN,
        }
        .fitted(1.0);
        assert!((t.start - 0.95).abs() < 1e-6 && t.end == 0.0, "{t:?}");
        assert!(
            (t.fade_in - 0.05).abs() < 1e-6 && t.fade_out == 0.0,
            "{t:?}"
        );
        let out = apply(
            &steady(1.0),
            &Trim {
                start: 5.0,
                end: 5.0,
                ..Default::default()
            },
        );
        assert_eq!(out.len_frames(), 50);
        // Negative values count as zero.
        let t = Trim {
            start: -1.0,
            ..Default::default()
        }
        .fitted(1.0);
        assert_eq!(t, Trim::default());
    }

    #[test]
    fn old_configs_without_it_load() {
        #[derive(Deserialize)]
        struct Entry {
            #[serde(default)]
            trim: Trim,
        }
        let e: Entry = serde_json::from_str("{}").unwrap();
        assert!(e.trim.is_none());
        let e: Entry = serde_json::from_str(r#"{"trim": {"start": 1.5}}"#).unwrap();
        assert_eq!(e.trim.start, 1.5);
        assert_eq!(e.trim.fade_out, 0.0);
    }
}
