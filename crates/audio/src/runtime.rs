use std::sync::Arc;

use fluidlite::{FontId, Synth};
use octava_core::{EventKind, PlaybackSnapshot, Ticks, TimedEvent};
use rtrb::{Consumer, Producer};

use crate::engine::{AudioCommand, AudioTelemetry, TransportState};

const TICK_EPSILON: f64 = 1.0e-9;

pub(crate) struct Runtime {
    synth: Synth,
    font_id: FontId,
    snapshot: Option<Arc<PlaybackSnapshot>>,
    state: TransportState,
    position_seconds: f64,
    end_seconds: f64,
    event_index: usize,
    sample_rate: f64,
    commands: Consumer<AudioCommand>,
    garbage: Producer<Arc<PlaybackSnapshot>>,
    telemetry: Arc<AudioTelemetry>,
    peak_left: f32,
    peak_right: f32,
}

impl Runtime {
    pub(crate) fn new(
        synth: Synth,
        font_id: FontId,
        sample_rate: u32,
        commands: Consumer<AudioCommand>,
        garbage: Producer<Arc<PlaybackSnapshot>>,
        telemetry: Arc<AudioTelemetry>,
    ) -> Self {
        let runtime = Self {
            synth,
            font_id,
            snapshot: None,
            state: TransportState::Stopped,
            position_seconds: 0.0,
            end_seconds: 0.0,
            event_index: 0,
            sample_rate: f64::from(sample_rate),
            commands,
            garbage,
            telemetry,
            peak_left: 0.0,
            peak_right: 0.0,
        };
        runtime.publish();
        runtime
    }

    pub(crate) fn render(&mut self, left: &mut [f32], right: &mut [f32]) -> usize {
        self.process_commands();

        if self.state != TransportState::Playing {
            left.fill(0.0);
            right.fill(0.0);
            self.peak_left = 0.0;
            self.peak_right = 0.0;
            self.publish();
            return 0;
        }

        let Some(snapshot) = self.snapshot.clone() else {
            self.state = TransportState::Stopped;
            left.fill(0.0);
            right.fill(0.0);
            self.peak_left = 0.0;
            self.peak_right = 0.0;
            self.publish();
            return 0;
        };

        let mut rendered = 0;
        while rendered < left.len() {
            if self.end_seconds <= TICK_EPSILON
                || self.position_seconds >= self.end_seconds - TICK_EPSILON
            {
                self.finish(&snapshot);
                break;
            }

            let available_frames = left.len() - rendered;
            let block_end = (self.position_seconds + available_frames as f64 / self.sample_rate)
                .min(self.end_seconds);
            let block_frames = (((block_end - self.position_seconds) * self.sample_rate).ceil()
                as usize)
                .min(available_frames);
            if block_frames == 0 {
                self.finish(&snapshot);
                break;
            }

            let next_event = snapshot.events.get(self.event_index).copied();
            let scheduled = next_event.and_then(|event| {
                let event_seconds = snapshot.tempo_map.seconds_at(event.tick);
                if event_seconds < block_end - TICK_EPSILON {
                    let offset = ((event_seconds - self.position_seconds) * self.sample_rate)
                        .round()
                        .max(0.0) as usize;
                    Some((offset.min(block_frames), event))
                } else {
                    None
                }
            });

            if let Some((offset, event)) = scheduled {
                if offset > 0 {
                    self.render_frames(
                        &mut left[rendered..rendered + offset],
                        &mut right[rendered..rendered + offset],
                    );
                    self.position_seconds += offset as f64 / self.sample_rate;
                    rendered += offset;
                }
                self.trigger(event);
                self.event_index += 1;
            } else {
                self.render_frames(
                    &mut left[rendered..rendered + block_frames],
                    &mut right[rendered..rendered + block_frames],
                );
                self.position_seconds += block_frames as f64 / self.sample_rate;
                rendered += block_frames;
            }

            if self.event_index >= snapshot.events.len()
                && self.position_seconds >= self.end_seconds - TICK_EPSILON
            {
                self.finish(&snapshot);
                break;
            }
        }

        if rendered < left.len() {
            left[rendered..].fill(0.0);
            right[rendered..].fill(0.0);
        }

        if self.state == TransportState::Playing {
            self.peak_left = peak(&left[..rendered]);
            self.peak_right = peak(&right[..rendered]);
        } else {
            self.peak_left = 0.0;
            self.peak_right = 0.0;
        }
        self.publish();
        rendered
    }

    pub(crate) fn render_tail(&mut self, left: &mut [f32], right: &mut [f32]) {
        let _ = self.synth.write((&mut *left, &mut *right));
        self.peak_left = peak(left);
        self.peak_right = peak(right);
        self.publish();
    }

    fn render_frames(&self, left: &mut [f32], right: &mut [f32]) {
        let _ = self.synth.write((left, right));
    }

    fn process_commands(&mut self) {
        while let Ok(command) = self.commands.pop() {
            match command {
                AudioCommand::Play { snapshot, start } => self.start(snapshot, start),
                AudioCommand::Resume => self.resume(),
                AudioCommand::Pause => self.state = TransportState::Paused,
                AudioCommand::Stop => self.stop(),
                AudioCommand::Seek(tick) => self.seek(tick),
                AudioCommand::SetSnapshot(snapshot) => self.replace_snapshot(snapshot),
            }
        }
    }

    fn start(&mut self, snapshot: Arc<PlaybackSnapshot>, start: Ticks) {
        let start = start.min(snapshot.length);
        self.retire_snapshot();
        self.position_seconds = snapshot.tempo_map.seconds_at(start);
        self.end_seconds = snapshot.tempo_map.seconds_at(snapshot.length);
        self.event_index = snapshot.events.partition_point(|event| event.tick < start);
        self.snapshot = Some(snapshot);
        self.reset_synth();
        self.state = if self.position_seconds < self.end_seconds - TICK_EPSILON {
            TransportState::Playing
        } else {
            TransportState::Paused
        };
    }

    fn resume(&mut self) {
        if self.snapshot.is_some() && self.position_seconds < self.end_seconds - TICK_EPSILON {
            self.state = TransportState::Playing;
        }
    }

    fn stop(&mut self) {
        self.state = TransportState::Stopped;
        self.position_seconds = 0.0;
        self.event_index = 0;
        self.reset_synth();
    }

    fn seek(&mut self, tick: Ticks) {
        let Some(snapshot) = self.snapshot.clone() else {
            self.position_seconds = 0.0;
            self.event_index = 0;
            return;
        };
        let tick = tick.min(snapshot.length);
        self.position_seconds = snapshot.tempo_map.seconds_at(tick);
        self.event_index = snapshot.events.partition_point(|event| event.tick < tick);
        self.reset_synth();
        if self.state == TransportState::Playing
            && self.position_seconds >= self.end_seconds - TICK_EPSILON
        {
            self.state = TransportState::Paused;
        }
    }

    fn replace_snapshot(&mut self, snapshot: Arc<PlaybackSnapshot>) {
        let tick = self.current_tick();
        self.retire_snapshot();
        self.position_seconds = snapshot.tempo_map.seconds_at(tick.min(snapshot.length));
        self.end_seconds = snapshot.tempo_map.seconds_at(snapshot.length);
        self.event_index = snapshot.events.partition_point(|event| event.tick < tick);
        self.snapshot = Some(snapshot);
        self.reset_synth();
    }

    fn retire_snapshot(&mut self) {
        if let Some(old) = self.snapshot.take()
            && let Err(rtrb::PushError::Full(old)) = self.garbage.push(old)
        {
            drop(old);
        }
    }

    fn reset_synth(&self) {
        let _ = self.synth.system_reset();
        if let Some(snapshot) = &self.snapshot {
            for track in &snapshot.tracks {
                let channel = u32::from(track.channel.min(15));
                let _ = self.synth.program_select(
                    channel,
                    self.font_id,
                    u32::from(track.preset.bank),
                    u32::from(track.preset.program),
                );
                let volume = (track.gain.clamp(0.0, 1.0) * 127.0).round() as u32;
                let pan = ((track.pan.clamp(-1.0, 1.0) + 1.0) * 63.5).round() as u32;
                let _ = self.synth.cc(channel, 7, volume);
                let _ = self.synth.cc(channel, 10, pan.min(127));
            }
        }
    }

    fn trigger(&self, event: TimedEvent) {
        let channel = u32::from(event.channel.min(15));
        match event.kind {
            EventKind::NoteOn { key, velocity } => {
                let _ = self
                    .synth
                    .note_on(channel, u32::from(key), u32::from(velocity));
            }
            EventKind::NoteOff { key } => {
                let _ = self.synth.note_off(channel, u32::from(key));
            }
        }
    }

    fn finish(&mut self, snapshot: &PlaybackSnapshot) {
        self.position_seconds = snapshot.tempo_map.seconds_at(snapshot.length);
        self.state = TransportState::Paused;
    }

    fn current_tick(&self) -> Ticks {
        self.snapshot.as_ref().map_or(Ticks::ZERO, |snapshot| {
            snapshot.tempo_map.tick_at_seconds(self.position_seconds)
        })
    }

    fn publish(&self) {
        let playhead = self.current_tick();
        self.telemetry
            .publish(self.state, playhead, self.peak_left, self.peak_right);
    }
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
        .min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::create_synth;
    use crate::soundfont::default_path;
    use octava_core::Project;
    use rtrb::RingBuffer;

    struct Harness {
        runtime: Runtime,
        commands: Producer<AudioCommand>,
        telemetry: Arc<AudioTelemetry>,
        _garbage: Consumer<Arc<PlaybackSnapshot>>,
    }

    fn harness() -> Harness {
        let sample_rate = 48_000;
        let (synth, font_id) = create_synth(&default_path(), sample_rate).unwrap();
        let telemetry = Arc::new(AudioTelemetry::new(sample_rate));
        let (commands, command_consumer) = RingBuffer::new(8);
        let (garbage, garbage_consumer) = RingBuffer::new(8);
        let runtime = Runtime::new(
            synth,
            font_id,
            sample_rate,
            command_consumer,
            garbage,
            Arc::clone(&telemetry),
        );
        Harness {
            runtime,
            commands,
            telemetry,
            _garbage: garbage_consumer,
        }
    }

    fn demo_snapshot() -> Arc<PlaybackSnapshot> {
        let mut project = Project::new("Audio test");
        let track = project.add_track("Piano").unwrap();
        let clip = project.add_clip(track, Ticks::ZERO, Ticks(1920)).unwrap();
        project
            .add_note(track, clip, 60, 110, Ticks::ZERO, Ticks(480))
            .unwrap();
        Arc::new(project.compile_snapshot())
    }

    fn render(runtime: &mut Runtime, blocks: usize) {
        let mut left = vec![0.0_f32; 512];
        let mut right = vec![0.0_f32; 512];
        for _ in 0..blocks {
            runtime.render(&mut left, &mut right);
        }
    }

    #[test]
    fn plays_until_the_end_of_the_snapshot() {
        let mut harness = harness();
        harness
            .commands
            .push(AudioCommand::Play {
                snapshot: demo_snapshot(),
                start: Ticks::ZERO,
            })
            .unwrap();

        render(&mut harness.runtime, 10);
        assert_eq!(harness.telemetry.state(), TransportState::Playing);
        assert!(harness.telemetry.playhead() > Ticks::ZERO);
        let (left, right) = harness.telemetry.peaks();
        assert!(left > 0.0 || right > 0.0);

        render(&mut harness.runtime, 100);
        assert_eq!(harness.telemetry.state(), TransportState::Paused);
        assert_eq!(harness.telemetry.playhead(), Ticks(1920));
    }

    #[test]
    fn pause_and_resume_keep_the_musical_position() {
        let mut harness = harness();
        harness
            .commands
            .push(AudioCommand::Play {
                snapshot: demo_snapshot(),
                start: Ticks::ZERO,
            })
            .unwrap();
        render(&mut harness.runtime, 4);
        let before = harness.telemetry.playhead();
        assert!(before > Ticks::ZERO);

        harness.commands.push(AudioCommand::Pause).unwrap();
        render(&mut harness.runtime, 2);
        assert_eq!(harness.telemetry.state(), TransportState::Paused);
        assert_eq!(harness.telemetry.playhead(), before);

        harness.commands.push(AudioCommand::Resume).unwrap();
        render(&mut harness.runtime, 2);
        assert_eq!(harness.telemetry.state(), TransportState::Playing);
        assert!(harness.telemetry.playhead() > before);
    }
}
