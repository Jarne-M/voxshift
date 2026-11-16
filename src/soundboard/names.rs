//! Display names for clips. A clip's name is separate from its file's name: on import the
//! file name is tidied up once ("freesound_community-180610-house-fly-close-buzzing-62408"
//! becomes "House Fly Close Buzzing"), and you can rename the clip to anything afterwards.
//! The file itself is never renamed.

use std::path::Path;

/// Where sound-effect sites put their own name in front of a download.
const SITE_PREFIXES: [&str; 6] = [
    "freesound_community",
    "freesound",
    "pixabay",
    "mixkit",
    "zapsplat",
    "soundbible",
];

fn is_number(token: &str) -> bool {
    !token.is_empty() && token.chars().all(|c| c.is_ascii_digit())
}

/// A download id that is not a number: a run of hex digits with both letters and digits in
/// it ("4a7d9c21"). Ordinary words never look like that.
fn is_hash(token: &str) -> bool {
    token.len() >= 8
        && token.chars().all(|c| c.is_ascii_hexdigit())
        && token.chars().any(|c| c.is_ascii_digit())
        && token.chars().any(|c| c.is_ascii_alphabetic())
}

/// "house" -> "House"; words that already have capitals ("XP", "iPhone") are left alone.
fn title_case(word: &str) -> String {
    if word.chars().any(char::is_uppercase) {
        return word.to_string();
    }
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A tidy display name for a file name without its extension: the site's prefix and the
/// download ids go, underscores and dashes become spaces, and every word gets a capital.
/// Never empty: a name that would clean away to nothing is kept as it is.
pub fn clean_name(stem: &str) -> String {
    let mut rest = stem.trim();
    let lower = rest.to_lowercase();
    for prefix in SITE_PREFIXES {
        // Only as a prefix followed by a separator: "freesound-x", not "freesounds".
        if let Some(after) = lower.strip_prefix(prefix)
            && after.starts_with(['-', '_', ' ', '.'])
        {
            rest = &rest[prefix.len()..];
            break;
        }
    }
    let mut words: Vec<&str> = rest
        .split(|c: char| c == '-' || c == '_' || c == '.' || c == '+' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .collect();
    // Ids in front (4 digits or more) and at the end (3 or more, or a hash). Small numbers
    // are part of the name: "take 2", "8 bit".
    while words.len() > 1 && is_number(words[0]) && words[0].len() >= 4 {
        words.remove(0);
    }
    while words.len() > 1
        && words
            .last()
            .is_some_and(|w| (is_number(w) && w.len() >= 3) || is_hash(w))
    {
        words.pop();
    }
    let name = words
        .iter()
        .map(|w| title_case(w))
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() {
        stem.trim().to_string()
    } else {
        name
    }
}

/// The file name of `path` without its extension, as older versions used for a clip's name.
pub fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sound".into())
}

/// The name a clip gets when its file is imported.
pub fn imported_name(path: &Path) -> String {
    clean_name(&file_stem(path))
}

/// A name typed by the user: trimmed and without line breaks; `None` if nothing is left.
pub fn typed_name(text: &str) -> Option<String> {
    let name: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_names_become_readable() {
        assert_eq!(
            clean_name("freesound_community-180610-house-fly-close-buzzing-62408"),
            "House Fly Close Buzzing"
        );
        assert_eq!(
            clean_name("vine_boom_sound_effect"),
            "Vine Boom Sound Effect"
        );
        assert_eq!(clean_name("pixabay-sad-trombone-12345"), "Sad Trombone");
        assert_eq!(
            clean_name("mixkit-arcade-retro-game-over-213"),
            "Arcade Retro Game Over"
        );
        assert_eq!(clean_name("airhorn"), "Airhorn");
        assert_eq!(clean_name("sound-effect-4a7d9c21"), "Sound Effect");
        assert_eq!(clean_name("  bruh  "), "Bruh");
    }

    #[test]
    fn what_belongs_to_the_name_stays() {
        // Capitals you (or the maker) put there, small numbers, accents.
        assert_eq!(clean_name("Windows XP shutdown"), "Windows XP Shutdown");
        assert_eq!(clean_name("take_2"), "Take 2");
        assert_eq!(clean_name("8-bit-jump"), "8 Bit Jump");
        assert_eq!(clean_name("\u{c9}coute"), "\u{c9}coute");
        assert_eq!(clean_name("clip07"), "Clip07");
        assert_eq!(clean_name("facade"), "Facade", "a word, not a hash");
        assert_eq!(clean_name("freesounds"), "Freesounds", "not a site prefix");
        // Nothing but an id: it is all the name there is.
        assert_eq!(clean_name("62408"), "62408");
        assert_eq!(clean_name("180610-62408"), "62408");
        assert_eq!(clean_name("---"), "---");
        assert_eq!(clean_name(""), "");
    }

    #[test]
    fn names_from_paths_and_from_typing() {
        let path = Path::new("C:/sounds/freesound_community-1234-wet_fart-99999.mp3");
        assert_eq!(file_stem(path), "freesound_community-1234-wet_fart-99999");
        assert_eq!(imported_name(path), "Wet Fart");
        assert_eq!(typed_name("  My   clip \n"), Some("My clip".to_string()));
        assert_eq!(
            typed_name("lower case stays"),
            Some("lower case stays".to_string())
        );
        assert_eq!(typed_name(" \t "), None);
    }
}
