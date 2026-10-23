//! Offline rendering for Octava.
//!
//! Rendering reuses the same scheduler and FluidLite synthesizer as real-time
//! playback. WAV output is stereo, 16-bit PCM.

#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::path::Path;
use std::sync::Arc;

use octava_audio::{AudioError, OfflineRenderer};
use octava_core::{PlaybackSnapshot, Project};
use thiserror::Error;

pub const DEFAULT_SAMPLE_RATE: u32 = 44_100;
pub const DEFAULT_TAIL_SECONDS: f32 = 1.0;

#[derive(Clone, Copy, Debug)]
pub struct RenderSettings {
    pub sample_rate: u32,
    pub tail_seconds: f32,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
            tail_seconds: DEFAULT_TAIL_SECONDS,
        }
    }
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("sample rate must be greater than zero")]
    InvalidSampleRate,
    #[error("tail length must be a finite, non-negative number of seconds")]
    InvalidTail,
    #[error(transparent)]
    Audio(#[from] AudioError),
    #[error("could not write WAV output: {0}")]
    Wav(#[from] hound::Error),
}

/// Compiles `project` and writes it as a stereo 16-bit WAV file.
pub fn render_wav(
    project: &Project,
    path: impl AsRef<Path>,
    settings: RenderSettings,
) -> Result<(), RenderError> {
    let snapshot = Arc::new(project.compile_snapshot());
    render_snapshot_wav_with(snapshot, path, settings, project.soundfont.as_deref())
}

/// Writes an already-compiled snapshot as a stereo 16-bit WAV file.
pub fn render_snapshot_wav(
    snapshot: Arc<PlaybackSnapshot>,
    path: impl AsRef<Path>,
    settings: RenderSettings,
) -> Result<(), RenderError> {
    render_snapshot_wav_with(snapshot, path, settings, None)
}

fn render_snapshot_wav_with(
    snapshot: Arc<PlaybackSnapshot>,
    path: impl AsRef<Path>,
    settings: RenderSettings,
    soundfont: Option<&Path>,
) -> Result<(), RenderError> {
    if settings.sample_rate == 0 {
        return Err(RenderError::InvalidSampleRate);
    }
    if !settings.tail_seconds.is_finite() || settings.tail_seconds < 0.0 {
        return Err(RenderError::InvalidTail);
    }

    let mut renderer = match soundfont {
        Some(soundfont) => OfflineRenderer::new_with_soundfont(settings.sample_rate, soundfont)?,
        None => OfflineRenderer::new(settings.sample_rate)?,
    };
    let samples = renderer.render(snapshot, settings.tail_seconds)?;
    write_wav(path.as_ref(), &samples, settings.sample_rate)
}

fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) -> Result<(), RenderError> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        writer.write_sample(value)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use octava_core::Ticks;

    use super::*;

    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

    fn temp_wav_path() -> PathBuf {
        let id = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("octava-render-{}-{id}.wav", std::process::id()))
    }

    fn demo_project() -> Project {
        let mut project = Project::new("Render test");
        let track = project.add_track("Piano").unwrap();
        let pattern = project.add_pattern("Clip", Ticks(1920)).unwrap();
        project.add_clip(track, pattern, Ticks::ZERO).unwrap();
        project
            .add_note(pattern, 60, 100, Ticks::ZERO, Ticks(960))
            .unwrap();
        project
    }

    #[test]
    fn renders_stereo_wav_with_tail() {
        let project = demo_project();
        let path = temp_wav_path();
        let settings = RenderSettings {
            sample_rate: 44_100,
            tail_seconds: 0.25,
        };

        render_wav(&project, &path, settings).unwrap();

        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().sample_rate, 44_100);
        assert_eq!(reader.spec().bits_per_sample, 16);

        let samples = reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            samples.len(),
            (project.tempo_map.samples_at(project.length_ticks(), 44_100) + 11_025) as usize * 2
        );
        assert!(samples.iter().any(|sample| *sample != 0));

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_invalid_settings() {
        let project = demo_project();
        let path = temp_wav_path();

        assert!(matches!(
            render_wav(
                &project,
                &path,
                RenderSettings {
                    sample_rate: 0,
                    ..RenderSettings::default()
                }
            ),
            Err(RenderError::InvalidSampleRate)
        ));
        assert!(matches!(
            render_wav(
                &project,
                &path,
                RenderSettings {
                    tail_seconds: f32::NAN,
                    ..RenderSettings::default()
                }
            ),
            Err(RenderError::InvalidTail)
        ));
    }
}
