//! Loading sound files (wav, mp3, ogg/vorbis, flac) into stereo f32 at the output rate.
//! Runs on the loader thread, never on the audio thread.

use super::mixer::SoundData;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use std::fs::File;
use std::path::Path;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// Longer files are cut (3 minutes of stereo at 48 kHz is ~70 MB of RAM).
pub const MAX_SECONDS: usize = 180;

pub const EXTENSIONS: [&str; 5] = ["wav", "mp3", "ogg", "flac", "oga"];

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

/// Decodes a file to interleaved stereo at its own sample rate.
pub fn decode_file(path: &Path) -> Result<(Vec<f32>, u32), String> {
    let file = File::open(path).map_err(|e| format!("cannot open: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| format!("unsupported file: {e}"))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or("no audio track")?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or("no audio codec parameters")?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| format!("unsupported codec: {e}"))?;

    let mut out = Vec::new();
    let mut packet_buf: Vec<f32> = Vec::new();
    let mut rate = 0u32;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymError::IoError(_)) => break, // truncated file: keep what we have
            Err(e) => return Err(format!("read error: {e}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            Err(SymError::DecodeError(_)) => continue, // skip a corrupt frame
            Err(e) => return Err(format!("decode error: {e}")),
        };
        rate = buf.spec().rate();
        let channels = buf.spec().channels().count().max(1);
        packet_buf.resize(buf.samples_interleaved(), 0.0);
        buf.copy_to_slice_interleaved(&mut packet_buf);
        for frame in packet_buf.chunks_exact(channels) {
            let (l, r) = if channels == 1 {
                (frame[0], frame[0])
            } else {
                (frame[0], frame[1])
            };
            out.push(l);
            out.push(r);
        }
        if rate > 0 && out.len() / 2 >= MAX_SECONDS * rate as usize {
            out.truncate(MAX_SECONDS * rate as usize * 2);
            break;
        }
    }
    if out.is_empty() || rate == 0 {
        return Err("file contains no audio".into());
    }
    Ok((out, rate))
}

/// High-quality offline resampling of interleaved stereo.
pub fn resample_stereo(frames: &[f32], from: u32, to: u32) -> Result<Vec<f32>, String> {
    if from == to || frames.is_empty() {
        return Ok(frames.to_vec());
    }
    let len = frames.len() / 2;
    let mut resampler = Fft::<f32>::new(from as usize, to as usize, 1024, 2, FixedSync::Input)
        .map_err(|e| format!("resampler: {e}"))?;
    let input = InterleavedSlice::new(frames, 2, len).map_err(|e| e.to_string())?;
    let out = resampler
        .process_all(&input, len, None)
        .map_err(|e| format!("resample: {e}"))?;
    Ok(out.take_data())
}

/// Decodes and (if `out_rate` is known) resamples to the output rate.
pub fn load(path: &Path, out_rate: u32) -> Result<SoundData, String> {
    let (frames, rate) = decode_file(path)?;
    if out_rate == 0 || out_rate == rate {
        return Ok(SoundData {
            frames,
            sample_rate: rate,
        });
    }
    Ok(SoundData {
        frames: resample_stereo(&frames, rate, out_rate)?,
        sample_rate: out_rate,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// Writes a 16-bit PCM WAV file.
    pub fn write_wav(path: &Path, rate: u32, channels: u16, samples: &[i16]) {
        let data_len = (samples.len() * 2) as u32;
        let mut f = File::create(path).unwrap();
        let block_align = channels * 2;
        f.write_all(b"RIFF").unwrap();
        f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
        f.write_all(b"WAVEfmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap();
        f.write_all(&channels.to_le_bytes()).unwrap();
        f.write_all(&rate.to_le_bytes()).unwrap();
        f.write_all(&(rate * u32::from(block_align)).to_le_bytes())
            .unwrap();
        f.write_all(&block_align.to_le_bytes()).unwrap();
        f.write_all(&16u16.to_le_bytes()).unwrap();
        f.write_all(b"data").unwrap();
        f.write_all(&data_len.to_le_bytes()).unwrap();
        for s in samples {
            f.write_all(&s.to_le_bytes()).unwrap();
        }
    }

    pub fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("voxshift-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn decodes_a_mono_wav_to_stereo() {
        let dir = temp_dir("decode");
        let path = dir.join("mono.wav");
        let samples: Vec<i16> = (0..4410)
            .map(|i| if i % 2 == 0 { 16384 } else { -16384 })
            .collect();
        write_wav(&path, 44_100, 1, &samples);
        let (frames, rate) = decode_file(&path).unwrap();
        assert_eq!(rate, 44_100);
        assert_eq!(frames.len(), 4410 * 2);
        assert!((frames[0] - 0.5).abs() < 1e-3 && (frames[1] - 0.5).abs() < 1e-3);
        assert!((frames[2] + 0.5).abs() < 1e-3);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn load_resamples_to_the_output_rate_keeping_duration() {
        let dir = temp_dir("resample");
        let path = dir.join("tone.wav");
        let samples: Vec<i16> = (0..44_100)
            .flat_map(|i| {
                let v = ((i as f32 * 0.05).sin() * 10_000.0) as i16;
                [v, v]
            })
            .collect();
        write_wav(&path, 44_100, 2, &samples);
        let sound = load(&path, 48_000).unwrap();
        assert_eq!(sound.sample_rate, 48_000);
        let frames = sound.len_frames();
        assert!((frames as i64 - 48_000).abs() < 100, "{frames} frames");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rejects_garbage_and_missing_files() {
        let dir = temp_dir("garbage");
        let path = dir.join("noise.mp3");
        std::fs::write(&path, b"definitely not audio").unwrap();
        assert!(load(&path, 48_000).is_err());
        assert!(load(&dir.join("missing.wav"), 48_000).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn extension_filter() {
        assert!(is_supported(Path::new("a/B.MP3")));
        assert!(is_supported(Path::new("x.flac")));
        assert!(!is_supported(Path::new("x.txt")));
        assert!(!is_supported(Path::new("noext")));
    }
}
