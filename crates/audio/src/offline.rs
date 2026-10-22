use std::path::Path;
use std::sync::Arc;

use octava_core::{PlaybackSnapshot, Ticks};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::engine::{AudioCommand, AudioError, AudioTelemetry, create_synth};
use crate::runtime::Runtime;
use crate::soundfont;

const COMMAND_CAPACITY: usize = 8;
const GARBAGE_CAPACITY: usize = 8;
const RENDER_BLOCK_FRAMES: usize = 1024;

/// Renders playback snapshots without an audio device.
///
/// The returned buffer is interleaved stereo (`L, R, L, R, ...`) using the
/// sample rate supplied at construction. Rendering reuses the same scheduler
/// and FluidLite synthesizer as real-time playback.
pub struct OfflineRenderer {
    runtime: Runtime,
    commands: Producer<AudioCommand>,
    _garbage: Consumer<Arc<PlaybackSnapshot>>,
    sample_rate: u32,
}

impl OfflineRenderer {
    pub fn new(sample_rate: u32) -> Result<Self, AudioError> {
        Self::new_with_soundfont(sample_rate, soundfont::default_path())
    }

    pub fn new_with_soundfont(
        sample_rate: u32,
        path: impl AsRef<Path>,
    ) -> Result<Self, AudioError> {
        if sample_rate == 0 {
            return Err(AudioError::InvalidSampleRate);
        }

        let (synth, font_id) = create_synth(path.as_ref(), sample_rate)?;
        let telemetry = Arc::new(AudioTelemetry::new(sample_rate));
        let (commands, command_consumer) = RingBuffer::new(COMMAND_CAPACITY);
        let (garbage, garbage_consumer) = RingBuffer::new(GARBAGE_CAPACITY);
        let runtime = Runtime::new(
            synth,
            font_id,
            sample_rate,
            command_consumer,
            garbage,
            telemetry,
        );

        Ok(Self {
            runtime,
            commands,
            _garbage: garbage_consumer,
            sample_rate,
        })
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Renders `snapshot` from the beginning, followed by `tail_seconds` of
    /// synthesizer release for notes that end at the project boundary.
    pub fn render(
        &mut self,
        snapshot: Arc<PlaybackSnapshot>,
        tail_seconds: f32,
    ) -> Result<Vec<f32>, AudioError> {
        if !tail_seconds.is_finite() || tail_seconds < 0.0 {
            return Err(AudioError::InvalidTailSeconds);
        }

        let musical_frames = usize::try_from(
            snapshot
                .tempo_map
                .samples_at(snapshot.length, self.sample_rate),
        )
        .unwrap_or(usize::MAX);
        let tail_frames = (f64::from(tail_seconds) * f64::from(self.sample_rate)).round() as usize;
        let total_frames = musical_frames.saturating_add(tail_frames);
        let mut output = Vec::with_capacity(total_frames.saturating_mul(2));

        self.commands
            .push(AudioCommand::Play {
                snapshot,
                start: Ticks::ZERO,
            })
            .map_err(|_| AudioError::CommandQueueFull)?;

        let mut left = vec![0.0_f32; RENDER_BLOCK_FRAMES];
        let mut right = vec![0.0_f32; RENDER_BLOCK_FRAMES];

        while output.len() / 2 < musical_frames {
            let remaining = musical_frames - output.len() / 2;
            let block = remaining.min(RENDER_BLOCK_FRAMES);
            let rendered = self.runtime.render(&mut left[..block], &mut right[..block]);
            push_interleaved(&mut output, &left[..rendered], &right[..rendered]);
            if rendered < block {
                break;
            }
        }

        if output.len() / 2 < musical_frames {
            output.resize(musical_frames.saturating_mul(2), 0.0);
        } else if output.len() / 2 > musical_frames {
            output.truncate(musical_frames.saturating_mul(2));
        }

        let mut tail_rendered = 0;
        while tail_rendered < tail_frames {
            let block = (tail_frames - tail_rendered).min(RENDER_BLOCK_FRAMES);
            self.runtime
                .render_tail(&mut left[..block], &mut right[..block]);
            push_interleaved(&mut output, &left[..block], &right[..block]);
            tail_rendered += block;
        }

        Ok(output)
    }
}

fn push_interleaved(output: &mut Vec<f32>, left: &[f32], right: &[f32]) {
    output.extend(
        left.iter()
            .zip(right)
            .flat_map(|(left, right)| [*left, *right]),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use octava_core::Project;

    #[test]
    fn note_off_on_the_project_boundary_is_delivered() {
        let mut project = Project::new("Boundary");
        let track = project.add_track("Piano").unwrap();
        let pattern = project.add_pattern("Clip", Ticks(1920)).unwrap();
        project.add_clip(track, pattern, Ticks::ZERO).unwrap();
        project
            .add_note(pattern, 60, 100, Ticks::ZERO, Ticks(1920))
            .unwrap();

        let mut renderer = OfflineRenderer::new(44_100).unwrap();
        renderer
            .render(Arc::new(project.compile_snapshot()), 0.1)
            .unwrap();

        assert_eq!(
            renderer.runtime.held_keys(),
            0,
            "the note ending on the last tick was never released"
        );
    }

    #[test]
    fn pan_moves_the_signal_between_channels() {
        let mut project = Project::new("Pan");
        let track = project.add_track("Piano").unwrap();
        project.track_mut(track).unwrap().pan = -1.0;
        let pattern = project.add_pattern("Clip", Ticks(1920)).unwrap();
        project.add_clip(track, pattern, Ticks::ZERO).unwrap();
        project
            .add_note(pattern, 60, 100, Ticks::ZERO, Ticks(960))
            .unwrap();

        let mut renderer = OfflineRenderer::new(44_100).unwrap();
        let output = renderer
            .render(Arc::new(project.compile_snapshot()), 0.1)
            .unwrap();

        let mut peak_left = 0.0_f32;
        let mut peak_right = 0.0_f32;
        for frame in output.as_chunks::<2>().0 {
            peak_left = peak_left.max(frame[0].abs());
            peak_right = peak_right.max(frame[1].abs());
        }
        assert!(
            peak_left > peak_right * 2.0,
            "panning left should favour the left channel: {peak_left} vs {peak_right}"
        );
    }
}
