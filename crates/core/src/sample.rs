//! Decoded audio samples used by the arrangement.
//!
//! Sample files are decoded on the UI thread and handed to the audio thread
//! inside a playback snapshot, so the real-time callback never touches the
//! filesystem.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;

/// Decoded, interleaved PCM kept in memory for playback.
#[derive(Clone, Debug, PartialEq)]
pub struct SampleData {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved samples in `-1.0..=1.0`.
    pub frames: Vec<f32>,
}

impl SampleData {
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.frames.len() / usize::from(self.channels.max(1))
    }

    #[must_use]
    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frame_count() as f64 / f64::from(self.sample_rate)
    }

    /// Returns the left and right sample for `frame`. Mono files are copied to
    /// both channels and files with more than two channels use the first two.
    #[must_use]
    pub fn stereo_frame(&self, frame: usize) -> (f32, f32) {
        let channels = usize::from(self.channels.max(1));
        let index = frame.saturating_mul(channels);
        let left = self.frames.get(index).copied().unwrap_or(0.0);
        let right = if channels > 1 {
            self.frames.get(index + 1).copied().unwrap_or(left)
        } else {
            left
        };
        (left, right)
    }
}

/// Header information read without decoding the whole file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SampleInfo {
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Error)]
pub enum SampleError {
    #[error("could not read sample '{path}': {source}")]
    Wav {
        path: String,
        #[source]
        source: hound::Error,
    },
    #[error("sample '{path}' contains no audio frames")]
    Empty { path: String },
    #[error("sample '{path}' uses an unsupported format: {bits}-bit {format}")]
    Unsupported {
        path: String,
        bits: u16,
        format: String,
    },
}

/// Reads a WAV file and converts it to normalized stereo-capable PCM.
pub fn load_sample(path: &Path) -> Result<SampleData, SampleError> {
    let mut reader = hound::WavReader::open(path).map_err(|source| SampleError::Wav {
        path: path.display().to_string(),
        source,
    })?;
    let spec = reader.spec();
    if spec.sample_rate == 0 || spec.channels == 0 {
        return Err(SampleError::Unsupported {
            path: path.display().to_string(),
            bits: spec.bits_per_sample,
            format: "zero sample rate or channel count".to_owned(),
        });
    }

    let path_label = path.display().to_string();
    let frames: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Int, 8) => collect(&mut reader, &path_label, |value: i8| {
            f32::from(value) / 128.0
        })?,
        (hound::SampleFormat::Int, 16) => collect(&mut reader, &path_label, |value: i16| {
            f32::from(value) / 32_768.0
        })?,
        (hound::SampleFormat::Int, 24) => collect(&mut reader, &path_label, |value: i32| {
            value as f32 / 8_388_608.0
        })?,
        (hound::SampleFormat::Int, 32) => collect(&mut reader, &path_label, |value: i32| {
            value as f32 / 2_147_483_648.0
        })?,
        (hound::SampleFormat::Float, 32) => collect(&mut reader, &path_label, |value: f32| value)?,
        (format, bits) => {
            return Err(SampleError::Unsupported {
                path: path_label,
                bits,
                format: format!("{format:?}"),
            });
        }
    };

    if frames.is_empty() {
        return Err(SampleError::Empty {
            path: path.display().to_string(),
        });
    }

    Ok(SampleData {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        frames,
    })
}

fn collect<R, S, F>(
    reader: &mut hound::WavReader<R>,
    path: &str,
    convert: F,
) -> Result<Vec<f32>, SampleError>
where
    R: std::io::Read,
    S: hound::Sample,
    F: Fn(S) -> f32,
{
    reader
        .samples::<S>()
        .map(|sample| {
            sample.map(&convert).map_err(|source| SampleError::Wav {
                path: path.to_owned(),
                source,
            })
        })
        .collect()
}

/// Reads only the WAV header so the file browser can show durations cheaply.
pub fn probe_sample(path: &Path) -> Result<SampleInfo, SampleError> {
    let reader = hound::WavReader::open(path).map_err(|source| SampleError::Wav {
        path: path.display().to_string(),
        source,
    })?;
    let spec = reader.spec();
    if spec.sample_rate == 0 {
        return Err(SampleError::Unsupported {
            path: path.display().to_string(),
            bits: spec.bits_per_sample,
            format: "zero sample rate".to_owned(),
        });
    }
    let frames = reader.duration();
    Ok(SampleInfo {
        duration_seconds: f64::from(frames) / f64::from(spec.sample_rate),
        sample_rate: spec.sample_rate,
        channels: spec.channels,
    })
}

/// Decoded samples keyed by path, shared between snapshot compilations.
///
/// Failures are cached too so that a missing file is not reopened on every
/// edit. Call [`SampleCache::clear`] after the files on disk change.
#[derive(Debug, Default)]
pub struct SampleCache {
    entries: HashMap<PathBuf, Option<Arc<SampleData>>>,
}

impl SampleCache {
    #[must_use]
    pub fn load(&mut self, path: &Path) -> Option<Arc<SampleData>> {
        if let Some(cached) = self.entries.get(path) {
            return cached.clone();
        }
        let loaded = load_sample(path).ok().map(Arc::new);
        self.entries.insert(path.to_path_buf(), loaded.clone());
        loaded
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, channels: u16, sample_rate: u32, samples: &[i16]) {
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for sample in samples {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn loads_mono_and_stereo_wavs() {
        let directory = std::env::temp_dir();
        let mono = directory.join(format!("octava-sample-mono-{}.wav", std::process::id()));
        write_wav(&mono, 1, 48_000, &[0, 16_384, -16_384, 0]);
        let data = load_sample(&mono).unwrap();
        assert_eq!(data.channels, 1);
        assert_eq!(data.frame_count(), 4);
        assert!((data.duration_seconds() - 4.0 / 48_000.0).abs() < 1.0e-9);
        assert_eq!(data.stereo_frame(1), (0.5, 0.5));

        let stereo = directory.join(format!("octava-sample-stereo-{}.wav", std::process::id()));
        write_wav(&stereo, 2, 44_100, &[16_384, -16_384, 0, 0]);
        let data = load_sample(&stereo).unwrap();
        assert_eq!(data.channels, 2);
        assert_eq!(data.frame_count(), 2);
        assert_eq!(data.stereo_frame(0), (0.5, -0.5));

        std::fs::remove_file(mono).unwrap();
        std::fs::remove_file(stereo).unwrap();
    }

    #[test]
    fn probe_reports_duration_without_decoding() {
        let path = std::env::temp_dir().join(format!("octava-probe-{}.wav", std::process::id()));
        write_wav(&path, 2, 48_000, &[0; 9_600]);
        let info = probe_sample(&path).unwrap();
        assert_eq!(info.channels, 2);
        assert_eq!(info.sample_rate, 48_000);
        assert!((info.duration_seconds - 0.1).abs() < 1.0e-9);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn cache_keeps_decoded_samples() {
        let path = std::env::temp_dir().join(format!("octava-cache-{}.wav", std::process::id()));
        write_wav(&path, 1, 48_000, &[0, 1, 2]);
        let mut cache = SampleCache::default();
        let first = cache.load(&path).unwrap();
        let second = cache.load(&path).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        cache.clear();
        assert!(cache.load(&path).is_some());
        std::fs::remove_file(path).unwrap();
    }
}
