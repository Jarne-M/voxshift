//! Short beeps for the headphones when something switches. The voice changer gets two tones
//! (rising for ON, falling for OFF); the monitor ("hear myself") gets a single tone (high for
//! ON, low for OFF), so the two are never confused. Each tone fades in and out so it never
//! clicks.

const TONE_S: f32 = 0.07;
const GAP_S: f32 = 0.025;
const FADE_S: f32 = 0.008;
const LOW_HZ: f32 = 660.0;
const HIGH_HZ: f32 = 990.0;
const MONITOR_ON_HZ: f32 = 1320.0;
const MONITOR_OFF_HZ: f32 = 440.0;

/// Which beep to play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CueKind {
    VoiceOn,
    VoiceOff,
    MonitorOn,
    MonitorOff,
}

impl CueKind {
    /// For passing through an atomic (0 is "nothing").
    pub fn code(self) -> u8 {
        match self {
            Self::VoiceOn => 1,
            Self::VoiceOff => 2,
            Self::MonitorOn => 3,
            Self::MonitorOff => 4,
        }
    }

    pub fn from_code(c: u8) -> Option<Self> {
        match c {
            1 => Some(Self::VoiceOn),
            2 => Some(Self::VoiceOff),
            3 => Some(Self::MonitorOn),
            4 => Some(Self::MonitorOff),
            _ => None,
        }
    }

    /// The tones (frequency per tone; one or two).
    fn tones(self) -> &'static [f32] {
        match self {
            Self::VoiceOn => &[LOW_HZ, HIGH_HZ],
            Self::VoiceOff => &[HIGH_HZ, LOW_HZ],
            Self::MonitorOn => &[MONITOR_ON_HZ],
            Self::MonitorOff => &[MONITOR_OFF_HZ],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Cue {
    fs: f32,
    /// Samples into the cue; `None` = silent.
    pos: Option<usize>,
    kind: CueKind,
    phase: f32,
}

impl Cue {
    pub fn new(fs: f32) -> Self {
        Self {
            fs,
            pos: None,
            kind: CueKind::VoiceOn,
            phase: 0.0,
        }
    }

    pub fn start(&mut self, kind: CueKind) {
        self.kind = kind;
        self.pos = Some(0);
        self.phase = 0.0;
    }

    pub fn active(&self) -> bool {
        self.pos.is_some()
    }

    fn len(&self) -> usize {
        let tones = self.kind.tones().len() as f32;
        ((tones * TONE_S + (tones - 1.0) * GAP_S) * self.fs) as usize
    }

    /// Next sample (mono, peak 1.0).
    #[inline]
    pub fn next(&mut self) -> f32 {
        let Some(pos) = self.pos else { return 0.0 };
        if pos >= self.len() {
            self.pos = None;
            return 0.0;
        }
        self.pos = Some(pos + 1);
        let t = pos as f32 / self.fs;
        let slot = TONE_S + GAP_S;
        let tone = (t / slot) as usize;
        let tone_t = t - tone as f32 * slot;
        if tone_t >= TONE_S {
            return 0.0; // the gap between two tones
        }
        let Some(&freq) = self.kind.tones().get(tone) else {
            return 0.0;
        };
        self.phase = (self.phase + freq / self.fs).fract();
        let env = (tone_t / FADE_S)
            .min((TONE_S - tone_t) / FADE_S)
            .clamp(0.0, 1.0);
        (std::f32::consts::TAU * self.phase).sin() * env
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::pitch::tests::frequency;

    fn render(kind: CueKind) -> Vec<f32> {
        let mut c = Cue::new(48_000.0);
        c.start(kind);
        let mut out = Vec::new();
        while c.active() {
            out.push(c.next());
        }
        out
    }

    fn assert_clean(v: &[f32]) {
        assert!(
            v[0].abs() < 0.01 && v.last().unwrap().abs() < 0.05,
            "no clicks"
        );
        assert!(v.iter().all(|x| x.abs() <= 1.0));
        let step = v
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(step < 0.2, "smooth: {step}");
    }

    #[test]
    fn on_rises_off_falls_and_both_end_cleanly() {
        let on = render(CueKind::VoiceOn);
        let off = render(CueKind::VoiceOff);
        assert!((on.len() as f32 / 48_000.0 - 0.165).abs() < 0.01);
        let half = |v: &[f32], first: bool| {
            let n = (TONE_S * 48_000.0) as usize;
            let part = if first { &v[..n] } else { &v[v.len() - n..] };
            frequency(&[part, part].concat(), 48_000.0)
        };
        assert!(half(&on, true) < half(&on, false), "ON goes up");
        assert!(half(&off, true) > half(&off, false), "OFF goes down");
        for v in [&on, &off] {
            assert_clean(v);
        }
    }

    #[test]
    fn monitor_beeps_are_single_tones_unlike_the_voice_beeps() {
        let on = render(CueKind::MonitorOn);
        let off = render(CueKind::MonitorOff);
        // One tone each: shorter than the two-tone voice cue.
        assert!((on.len() as f32 / 48_000.0 - TONE_S).abs() < 0.002);
        assert_eq!(on.len(), off.len());
        let f_on = frequency(&[&on[..], &on[..]].concat(), 48_000.0);
        let f_off = frequency(&[&off[..], &off[..]].concat(), 48_000.0);
        assert!(
            f_on > 1200.0 && f_off < 500.0,
            "high ON {f_on}, low OFF {f_off}"
        );
        for v in [&on, &off] {
            assert_clean(v);
        }
        for k in [
            CueKind::VoiceOn,
            CueKind::VoiceOff,
            CueKind::MonitorOn,
            CueKind::MonitorOff,
        ] {
            assert_eq!(CueKind::from_code(k.code()), Some(k));
        }
        assert_eq!(CueKind::from_code(0), None);
    }
}
