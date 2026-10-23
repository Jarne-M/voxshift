//! Preset packs: your presets and the MIDI files they sing, in one file to send to a friend.
//!
//! A `.voxpack` is JSON: the presets, plus the bytes of each MIDI file (base64). Inside the
//! pack, a preset names its MIDI file by file name only; importing writes the files into the
//! friend's VoxShift folder and points the presets there. Name clashes with presets they
//! already have are resolved with "(2)", "(3)", ... and identical presets are not doubled.

use super::presets::{self, Preset};
use crate::melody::{MelodySource, is_midi};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "voxpack";
/// Version of the file format this build writes (and the newest it can read).
const FORMAT: u32 = 1;
/// MIDI files bigger than this are left out (a melody is a few kilobytes).
const MAX_FILE: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pack {
    pub format: u32,
    pub presets: Vec<Preset>,
    pub melodies: Vec<PackedFile>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PackedFile {
    /// File name, e.g. "my song.mid".
    pub name: String,
    /// The file's bytes, base64.
    pub data: String,
}

pub fn is_pack(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// Builds a pack from your presets and MIDI files (the melody list plus any file a preset
/// uses). Returns the pack and what had to be left out, described for the user.
pub fn export(presets: &[Preset], files: &[PathBuf]) -> (Pack, Vec<String>) {
    let mut paths: Vec<PathBuf> = files.to_vec();
    for p in presets {
        if let Some(MelodySource::File { path, .. }) = &p.melody
            && !paths.contains(path)
        {
            paths.push(path.clone());
        }
    }
    let mut skipped = Vec::new();
    let mut melodies: Vec<PackedFile> = Vec::new();
    let mut packed_as: HashMap<PathBuf, String> = HashMap::new();
    for path in paths {
        let shown = path.display().to_string();
        match std::fs::read(&path) {
            Ok(bytes) if bytes.len() > MAX_FILE => skipped.push(format!("{shown} (too big)")),
            Ok(bytes) => {
                let name = unique_name(&file_name(&path), |n| melodies.iter().any(|m| m.name == n));
                packed_as.insert(path, name.clone());
                melodies.push(PackedFile {
                    name,
                    data: base64_encode(&bytes),
                });
            }
            Err(_) => skipped.push(format!("{shown} (file not found)")),
        }
    }
    let presets = presets
        .iter()
        .map(|p| {
            let mut p = p.clone();
            if let Some(MelodySource::File { path, part }) = &p.melody {
                p.melody = packed_as.get(path).map(|name| MelodySource::File {
                    path: PathBuf::from(name),
                    part: *part,
                });
            }
            p
        })
        .collect();
    let pack = Pack {
        format: FORMAT,
        presets,
        melodies,
    };
    (pack, skipped)
}

pub fn save(path: &Path, pack: &Pack) -> Result<(), String> {
    let json = serde_json::to_string_pretty(pack).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| format!("could not write {}: {e}", path.display()))
}

pub fn load(path: &Path) -> Result<Pack, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let pack: Pack =
        serde_json::from_str(&text).map_err(|_| "not a VoxShift preset pack".to_string())?;
    if pack.format > FORMAT {
        return Err("made by a newer VoxShift: update VoxShift to open it".into());
    }
    Ok(pack)
}

/// What an import brought in.
#[derive(Debug, Default, PartialEq)]
pub struct Imported {
    /// New presets to add (renamed where needed, MIDI paths pointing into `dir`).
    pub presets: Vec<Preset>,
    /// MIDI files now in `dir`.
    pub files: Vec<PathBuf>,
    /// Presets left out because you already have exactly the same one.
    pub already_had: usize,
}

/// Writes the pack's MIDI files into `dir` and returns its presets, ready to add next to
/// `existing`.
pub fn import(pack: &Pack, existing: &[Preset], dir: &Path) -> Result<Imported, String> {
    let mut out = Imported::default();
    let mut written: HashMap<&str, PathBuf> = HashMap::new();
    if !pack.melodies.is_empty() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    for m in &pack.melodies {
        let bytes = base64_decode(&m.data).ok_or("the pack is damaged")?;
        // Only a plain file name, never a path, and always a MIDI file.
        let mut name = file_name(Path::new(&m.name));
        if !is_midi(Path::new(&name)) {
            name.push_str(".mid");
        }
        // The same file already there (imported before): reuse it. Otherwise a free name.
        let same = |n: &str| std::fs::read(dir.join(n)).is_ok_and(|b| b == bytes);
        let path = if same(&name) {
            dir.join(&name)
        } else {
            let name = unique_name(&name, |n| dir.join(n).exists() && !same(n));
            let path = dir.join(&name);
            if !same(&name) {
                std::fs::write(&path, &bytes)
                    .map_err(|e| format!("could not write {}: {e}", path.display()))?;
            }
            path
        };
        written.insert(&m.name, path.clone());
        out.files.push(path);
    }
    for p in &pack.presets {
        let mut p = p.clone();
        if let Some(MelodySource::File { path, part }) = &p.melody {
            p.melody =
                written
                    .get(path.to_string_lossy().as_ref())
                    .map(|path| MelodySource::File {
                        path: path.clone(),
                        part: *part,
                    });
        }
        let taken = |n: &str| {
            presets::is_builtin(n)
                || existing
                    .iter()
                    .chain(&out.presets)
                    .any(|e| e.name.eq_ignore_ascii_case(n))
        };
        // Already here, under its own name or a "(2)" copy from an earlier import.
        let copy_of = |e: &Preset| {
            e.name == p.name
                || e.name
                    .strip_prefix(p.name.as_str())
                    .is_some_and(|rest| rest.starts_with(" (") && rest.ends_with(')'))
        };
        if existing
            .iter()
            .any(|e| copy_of(e) && e.effects == p.effects && e.melody == p.melody)
        {
            out.already_had += 1;
            continue;
        }
        if taken(&p.name) {
            p.name = unique_name(&p.name, taken);
        }
        out.presets.push(p);
    }
    Ok(out)
}

/// The last part of a path ("C:/a/b/song.mid" -> "song.mid").
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "melody.mid".into())
}

/// `name`, or "name (2)", "name (3)", ... (before the extension for file names) until
/// `taken` says no.
fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if is_midi(Path::new(name)) => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    (2..)
        .map(|i| format!("{stem} ({i}){ext}"))
        .find(|n| !taken(n))
        .expect("some number is free")
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(B64[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
    {
        let v = B64.iter().position(|&b| b == c)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soundboard::decode::tests::temp_dir;

    fn preset(name: &str, melody: Option<MelodySource>) -> Preset {
        Preset {
            name: name.into(),
            effects: presets::builtin()[1].effects.clone(),
            melody,
        }
    }

    #[test]
    fn base64_round_trips() {
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(base64_encode(b"M"), "TQ==");
        for len in 0..300 {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(base64_decode(&base64_encode(&bytes)).unwrap(), bytes);
        }
        assert!(base64_decode("not*base64").is_none());
    }

    #[test]
    fn a_pack_carries_presets_and_their_midi_files_to_a_friend() {
        let mine = temp_dir("pack-mine");
        let theirs = temp_dir("pack-theirs");
        let song = mine.join("our song.mid");
        let other = mine.join("sub").join("our song.mid"); // same name, other file
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        std::fs::write(&song, b"MThd song one").unwrap();
        std::fs::write(&other, b"MThd song two").unwrap();
        let presets = vec![
            preset(
                "Party",
                Some(MelodySource::File {
                    path: song.clone(),
                    part: Some(2),
                }),
            ),
            preset("Plain", None),
            preset(
                "Gone",
                Some(MelodySource::File {
                    path: mine.join("deleted.mid"),
                    part: None,
                }),
            ),
            preset("Ode", Some(MelodySource::BuiltIn("Ode to Joy".into()))),
        ];
        let (pack, skipped) = export(&presets, std::slice::from_ref(&other));
        assert_eq!(pack.melodies.len(), 2);
        assert_eq!(skipped.len(), 1, "{skipped:?}");
        assert!(skipped[0].contains("deleted.mid"));
        let path = mine.join("share.voxpack");
        assert!(is_pack(&path));
        save(&path, &pack).unwrap();

        // On the friend's PC: they already have a preset called "Party".
        let loaded = load(&path).unwrap();
        assert_eq!(loaded, pack);
        let existing = vec![preset("party", None)];
        let got = import(&loaded, &existing, &theirs).unwrap();
        let names: Vec<&str> = got.presets.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Party (2)", "Plain", "Gone", "Ode"]);
        assert_eq!(got.files.len(), 2);
        assert!(got.files.iter().all(|f| f.starts_with(&theirs)));
        // The preset points at the friend's copy of the right file, with its part.
        let Some(MelodySource::File { path, part }) = &got.presets[0].melody else {
            panic!("melody lost")
        };
        assert_eq!(std::fs::read(path).unwrap(), b"MThd song one");
        assert_eq!(*part, Some(2));
        assert_eq!(got.presets[2].melody, None, "its file was missing");
        assert_eq!(
            got.presets[3].melody, presets[3].melody,
            "built-ins travel as names"
        );

        // Importing the same pack again adds nothing new and does not duplicate the files.
        let mut all = existing.clone();
        all.extend(got.presets.iter().cloned());
        let again = import(&loaded, &all, &theirs).unwrap();
        assert_eq!(again.already_had, 4);
        assert!(again.presets.is_empty());
        assert_eq!(std::fs::read_dir(&theirs).unwrap().count(), 2);
        std::fs::remove_dir_all(mine).ok();
        std::fs::remove_dir_all(theirs).ok();
    }

    #[test]
    fn hostile_or_broken_packs_are_refused_or_tamed() {
        let dir = temp_dir("pack-bad");
        let path = dir.join("x.voxpack");
        std::fs::write(&path, "{ nope").unwrap();
        assert!(
            load(&path)
                .unwrap_err()
                .contains("not a VoxShift preset pack")
        );
        std::fs::write(&path, r#"{"format":99,"presets":[],"melodies":[]}"#).unwrap();
        assert!(load(&path).unwrap_err().contains("newer VoxShift"));
        // A file name trying to escape the folder is reduced to a plain name.
        let pack = Pack {
            format: 1,
            presets: vec![preset(
                "Clean",
                Some(MelodySource::File {
                    path: "../../evil.txt".into(),
                    part: None,
                }),
            )],
            melodies: vec![PackedFile {
                name: "../../evil.txt".into(),
                data: base64_encode(b"MThd"),
            }],
        };
        let out = dir.join("melodies");
        let got = import(&pack, &[], &out).unwrap();
        assert_eq!(got.files, vec![out.join("evil.txt.mid")]);
        assert_eq!(
            got.presets[0].name, "Clean (2)",
            "built-in names are not taken over"
        );
        assert!(
            matches!(&got.presets[0].melody, Some(MelodySource::File { path, .. }) if path.starts_with(&out))
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
