//! Finding a device again by name. Indexes change between reboots and houses, and Windows
//! renames devices slightly ("Microphone (2- HyperX Cloud II)" after re-plugging into another
//! port), so we match on normalised words instead of exact strings.

/// Lowercase words, without Windows' "2- " duplicate prefixes and punctuation.
pub fn normalize(name: &str) -> Vec<String> {
    let lower = name.to_lowercase();
    let mut words = Vec::new();
    for raw in lower.split(|c: char| !c.is_alphanumeric()) {
        if raw.is_empty() {
            continue;
        }
        words.push(raw.to_string());
    }
    // "Microphone (2- USB Mic)" or "1 - HP X24ih (...)": a bare number at the start or right
    // after "(", followed by "-", is the Windows duplicate counter, not part of the name.
    let mut cleaned = Vec::with_capacity(words.len());
    let mut chars_seen = 0;
    for w in words {
        let start = lower[chars_seen..].find(w.as_str()).map(|i| i + chars_seen);
        let is_counter = w.chars().all(|c| c.is_ascii_digit())
            && start.is_some_and(|s| {
                let before = lower[..s].trim_end();
                let after = lower[s + w.len()..].trim_start();
                (before.is_empty() || before.ends_with('(')) && after.starts_with('-')
            });
        if let Some(s) = start {
            chars_seen = s + w.len();
        }
        if !is_counter {
            cleaned.push(w);
        }
    }
    cleaned
}

/// 0..=1: how alike two device names are. 1 = same words.
pub fn similarity(a: &str, b: &str) -> f32 {
    let (wa, wb) = (normalize(a), normalize(b));
    if wa.is_empty() || wb.is_empty() {
        return 0.0;
    }
    if wa == wb {
        return 1.0;
    }
    let common = wa.iter().filter(|w| wb.contains(w)).count() as f32;
    let union = (wa.len() + wb.len()) as f32 - common;
    let jaccard = common / union;
    // One name containing every word of the other ("Microphone (HyperX Cloud II)" vs
    // "Headset Microphone (HyperX Cloud II)") is a strong hint.
    let shorter = wa.len().min(wb.len()) as f32;
    let containment = if common == shorter { 0.85 } else { 0.0 };
    jaccard.max(containment)
}

/// Minimum similarity to accept a device as "the same one".
pub const THRESHOLD: f32 = 0.6;

/// The candidate that best matches `wanted` (exact name first), if any is close enough.
pub fn best_match<'a>(wanted: &str, candidates: &'a [String]) -> Option<&'a String> {
    if wanted.trim().is_empty() {
        return None;
    }
    if let Some(exact) = candidates.iter().find(|c| c.as_str() == wanted) {
        return Some(exact);
    }
    candidates
        .iter()
        .map(|c| (similarity(wanted, c), c))
        .filter(|(s, _)| *s >= THRESHOLD)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c)| c)
}

/// The VB-CABLE playback endpoint, if installed ("CABLE Input (VB-Audio Virtual Cable)").
pub fn find_cable_input(outputs: &[String]) -> Option<&String> {
    outputs
        .iter()
        .find(|n| n.to_lowercase().starts_with("cable input"))
        .or_else(|| {
            outputs
                .iter()
                .find(|n| n.to_lowercase().contains("cable input"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn normalize_drops_windows_counters_but_keeps_real_numbers() {
        assert_eq!(
            normalize("Microphone (2- HyperX Cloud II)"),
            ["microphone", "hyperx", "cloud", "ii"]
        );
        assert_eq!(normalize("Mic (Yeti 2)"), ["mic", "yeti", "2"]);
        assert_eq!(
            normalize("1 - HP X24ih (AMD High Definition Audio Device)"),
            normalize("HP X24ih (AMD High Definition Audio Device)")
        );
        assert_eq!(
            normalize("CABLE Input (VB-Audio Virtual Cable)"),
            ["cable", "input", "vb", "audio", "virtual", "cable"]
        );
    }

    #[test]
    fn finds_renamed_devices() {
        let inputs = names(&[
            "Microphone (Realtek(R) Audio)",
            "Microphone (3- HyperX Cloud II Wireless)",
            "CABLE Output (VB-Audio Virtual Cable)",
        ]);
        assert_eq!(
            best_match("Microphone (HyperX Cloud II Wireless)", &inputs).unwrap(),
            &inputs[1]
        );
        assert_eq!(
            best_match("Headset Microphone (HyperX Cloud II Wireless)", &inputs).unwrap(),
            &inputs[1]
        );
        assert_eq!(
            best_match("Microphone (Realtek(R) Audio)", &inputs).unwrap(),
            &inputs[0]
        );
    }

    #[test]
    fn does_not_confuse_different_devices() {
        let inputs = names(&["Microphone (Realtek(R) Audio)", "Microphone (Blue Yeti)"]);
        assert!(best_match("Microphone (HyperX Cloud II)", &inputs).is_none());
        assert!(best_match("", &inputs).is_none());
    }

    #[test]
    fn exact_name_wins_over_similar_ones() {
        let inputs = names(&["Microphone (USB Audio)", "Microphone (2- USB Audio)"]);
        assert_eq!(
            best_match("Microphone (2- USB Audio)", &inputs).unwrap(),
            &inputs[1]
        );
    }

    #[test]
    fn detects_vb_cable() {
        let outputs = names(&[
            "Speakers (Realtek(R) Audio)",
            "CABLE In 16ch (VB-Audio Virtual Cable)",
            "CABLE Input (VB-Audio Virtual Cable)",
        ]);
        assert_eq!(find_cable_input(&outputs).unwrap(), &outputs[2]);
        assert!(find_cable_input(&outputs[..1]).is_none());
    }
}
