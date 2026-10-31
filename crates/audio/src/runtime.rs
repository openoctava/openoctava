use std::sync::Arc;

use fluidlite::{FontId, Synth};
use octava_core::{EventKind, PlaybackSnapshot, Preset, SampleData, Ticks, TimedEvent};
use rtrb::{Consumer, Producer};

use crate::engine::{AudioCommand, AudioTelemetry, PreviewNote, TransportState};

const TICK_EPSILON: f64 = 1.0e-9;
const PREVIEW_SECONDS: f64 = 0.4;
const PREVIEW_NOTE_OFF_SECONDS: f64 = 0.24;

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
    preview_frames: usize,
    preview_note_off_frames: usize,
    preview_channel: u32,
    preview_key: u32,
    preview_released: bool,
    /// Bitmask of the keys currently held on each channel.
    active: [u128; 16],
    /// Program currently selected on each channel, so that patterns using a
    /// different instrument can switch the channel without redundant calls.
    channel_preset: [Option<Preset>; 16],
    /// Index of the next sample that has not started yet.
    sample_index: usize,
    /// Samples currently being mixed into the output.
    sample_voices: Vec<SampleVoice>,
    /// Sample auditioned from the Files panel while the transport is stopped.
    preview_sample: Option<SampleVoice>,
}

/// One playing audio sample, with its own resampling position.
struct SampleVoice {
    data: Arc<SampleData>,
    /// Timeline start of the placement this voice belongs to.
    start: Ticks,
    /// Position in source frames.
    position: f64,
    gain_left: f32,
    gain_right: f32,
}

impl SampleVoice {
    fn new(data: Arc<SampleData>, start: Ticks, gain: f32, pan: f32) -> Self {
        let gain = gain.clamp(0.0, 1.0);
        let pan = pan.clamp(-1.0, 1.0);
        Self {
            data,
            start,
            position: 0.0,
            gain_left: gain * if pan > 0.0 { 1.0 - pan } else { 1.0 },
            gain_right: gain * if pan < 0.0 { 1.0 + pan } else { 1.0 },
        }
    }

    fn with_offset(
        data: Arc<SampleData>,
        start: Ticks,
        gain: f32,
        pan: f32,
        offset_frames: f64,
    ) -> Self {
        let mut voice = Self::new(data, start, gain, pan);
        voice.position = offset_frames.max(0.0);
        voice
    }

    fn finished(&self) -> bool {
        self.position >= self.data.frame_count() as f64
    }

    fn mix_into(&mut self, left: &mut [f32], right: &mut [f32], step: f64) {
        let frames = self.data.frame_count();
        if frames == 0 {
            self.position = 0.0;
            return;
        }
        let last = frames - 1;
        let step = step.max(f64::MIN_POSITIVE);
        let mut position = self.position;
        for index in 0..left.len() {
            if position >= frames as f64 {
                break;
            }
            let base = position.floor() as usize;
            let fraction = (position - base as f64) as f32;
            let (left_a, right_a) = self.data.stereo_frame(base.min(last));
            let (left_b, right_b) = self.data.stereo_frame((base + 1).min(last));
            left[index] += (left_a + (left_b - left_a) * fraction) * self.gain_left;
            right[index] += (right_a + (right_b - right_a) * fraction) * self.gain_right;
            position += step;
        }
        self.position = position;
    }
}

const CHANNELS: usize = 16;

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
            preview_frames: 0,
            preview_note_off_frames: 0,
            preview_channel: 0,
            preview_key: 0,
            preview_released: true,
            active: [0; CHANNELS],
            channel_preset: [None; CHANNELS],
            sample_index: 0,
            sample_voices: Vec::new(),
            preview_sample: None,
        };
        runtime.publish();
        runtime
    }

    pub(crate) fn render(&mut self, left: &mut [f32], right: &mut [f32]) -> usize {
        self.process_commands();

        if self.state != TransportState::Playing {
            return self.render_preview(left, right);
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
            let next_sample = snapshot.samples.get(self.sample_index);
            let next = match (next_event, next_sample) {
                (Some(event), Some(sample)) => {
                    let event_seconds = snapshot.tempo_map.seconds_at(event.tick);
                    let sample_seconds = snapshot.tempo_map.seconds_at(sample.start);
                    if event_seconds <= sample_seconds {
                        Some((event_seconds, true))
                    } else {
                        Some((sample_seconds, false))
                    }
                }
                (Some(event), None) => Some((snapshot.tempo_map.seconds_at(event.tick), true)),
                (None, Some(sample)) => Some((snapshot.tempo_map.seconds_at(sample.start), false)),
                (None, None) => None,
            };
            let scheduled = next.and_then(|(seconds, is_event)| {
                // `<=` matters: a note-off that lands exactly on the project
                // boundary is clamped to `block_end`, and dropping it would
                // leave the voice sounding through the whole release tail.
                if seconds <= block_end + TICK_EPSILON {
                    let offset = ((seconds - self.position_seconds) * self.sample_rate)
                        .round()
                        .max(0.0) as usize;
                    Some((offset.min(block_frames), is_event))
                } else {
                    None
                }
            });

            if let Some((offset, is_event)) = scheduled {
                if offset > 0 {
                    self.render_frames(
                        &mut left[rendered..rendered + offset],
                        &mut right[rendered..rendered + offset],
                    );
                    self.position_seconds += offset as f64 / self.sample_rate;
                    rendered += offset;
                }
                if is_event {
                    self.trigger(snapshot.events[self.event_index]);
                    self.event_index += 1;
                } else {
                    let sample = &snapshot.samples[self.sample_index];
                    self.sample_voices.push(SampleVoice::new(
                        Arc::clone(&sample.data),
                        sample.start,
                        sample.gain,
                        sample.pan,
                    ));
                    self.sample_index += 1;
                }
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
        self.mix_sample_voices(left, right);
        self.peak_left = peak(left);
        self.peak_right = peak(right);
        self.publish();
    }

    /// Number of keys the synthesizer still holds. Every note must have
    /// received its note-off once playback has finished.
    #[cfg(test)]
    pub(crate) fn held_keys(&self) -> u32 {
        self.active.iter().map(|mask| mask.count_ones()).sum()
    }

    fn render_frames(&mut self, left: &mut [f32], right: &mut [f32]) {
        let _ = self.synth.write((&mut *left, &mut *right));
        self.mix_sample_voices(left, right);
    }

    fn mix_sample_voices(&mut self, left: &mut [f32], right: &mut [f32]) {
        for voice in &mut self.sample_voices {
            let step = f64::from(voice.data.sample_rate) / self.sample_rate;
            voice.mix_into(left, right, step);
        }
        self.sample_voices.retain(|voice| !voice.finished());
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
                AudioCommand::PreviewNote(note) => self.preview_note(note),
                AudioCommand::PreviewSample(data) => self.start_sample_preview(data),
                AudioCommand::LoadSoundFont(path) => self.load_soundfont(path),
            }
        }
    }

    fn start(&mut self, snapshot: Arc<PlaybackSnapshot>, start: Ticks) {
        let start = start.min(snapshot.length);
        self.clear_preview();
        self.retire_snapshot();
        self.position_seconds = snapshot.tempo_map.seconds_at(start);
        self.end_seconds = snapshot.tempo_map.seconds_at(snapshot.length);
        self.event_index = snapshot.events.partition_point(|event| event.tick < start);
        self.snapshot = Some(snapshot);
        self.reset_synth();
        self.schedule_samples(start);
        self.state = if self.position_seconds < self.end_seconds - TICK_EPSILON {
            TransportState::Playing
        } else {
            TransportState::Paused
        };
    }

    /// Clears the sample voices and starts every sample that is already
    /// sounding at `start` from the correct offset.
    fn schedule_samples(&mut self, start: Ticks) {
        self.sample_voices.clear();
        let Some(snapshot) = self.snapshot.clone() else {
            self.sample_index = 0;
            return;
        };
        let start_seconds = snapshot.tempo_map.seconds_at(start);
        self.sample_index = snapshot
            .samples
            .partition_point(|sample| sample.start < start);
        for sample in &snapshot.samples[..self.sample_index] {
            let sample_start = snapshot.tempo_map.seconds_at(sample.start);
            let offset_seconds = start_seconds - sample_start;
            if offset_seconds < 0.0 {
                continue;
            }
            let offset_frames = offset_seconds * f64::from(sample.data.sample_rate);
            if offset_frames < sample.data.frame_count() as f64 {
                self.sample_voices.push(SampleVoice::with_offset(
                    Arc::clone(&sample.data),
                    sample.start,
                    sample.gain,
                    sample.pan,
                    offset_frames,
                ));
            }
        }
    }

    fn resume(&mut self) {
        if self.snapshot.is_some() && self.position_seconds < self.end_seconds - TICK_EPSILON {
            self.clear_preview();
            self.reset_synth();
            self.state = TransportState::Playing;
        }
    }

    fn stop(&mut self) {
        self.state = TransportState::Stopped;
        self.position_seconds = 0.0;
        self.event_index = 0;
        self.sample_index = 0;
        self.sample_voices.clear();
        self.clear_preview();
        self.reset_synth();
    }

    fn seek(&mut self, tick: Ticks) {
        self.clear_preview();
        let Some(snapshot) = self.snapshot.clone() else {
            self.position_seconds = 0.0;
            self.event_index = 0;
            self.sample_index = 0;
            self.sample_voices.clear();
            self.reset_synth();
            return;
        };
        let tick = tick.min(snapshot.length);
        self.position_seconds = snapshot.tempo_map.seconds_at(tick);
        self.event_index = snapshot.events.partition_point(|event| event.tick < tick);
        self.reset_synth();
        self.schedule_samples(tick);
        if self.state == TransportState::Playing
            && self.position_seconds >= self.end_seconds - TICK_EPSILON
        {
            self.state = TransportState::Paused;
        }
    }

    fn replace_snapshot(&mut self, snapshot: Arc<PlaybackSnapshot>) {
        let tick = self.current_tick();
        let start = tick.min(snapshot.length);
        let event_index = snapshot.events.partition_point(|event| event.tick < start);

        self.clear_preview();
        self.retire_snapshot();
        self.position_seconds = snapshot.tempo_map.seconds_at(start);
        self.end_seconds = snapshot.tempo_map.seconds_at(snapshot.length);
        self.event_index = event_index;
        self.sample_index = snapshot
            .samples
            .partition_point(|sample| sample.start < start);
        // Keep samples that are still sounding and still present in the new
        // snapshot, so that moving a fader does not cut them.
        let previous = std::mem::take(&mut self.sample_voices);
        self.snapshot = Some(snapshot);
        let current = self.snapshot.clone();
        if let Some(snapshot) = current {
            for voice in previous {
                let Some(sample) = snapshot.samples.iter().find(|sample| {
                    sample.start == voice.start && Arc::ptr_eq(&sample.data, &voice.data)
                }) else {
                    continue;
                };
                self.sample_voices.push(SampleVoice::with_offset(
                    Arc::clone(&sample.data),
                    sample.start,
                    sample.gain,
                    sample.pan,
                    voice.position,
                ));
            }
        }
        // Do not reset the synthesizer: edits are applied while playback
        // continues, so the voices that are currently sounding must survive.
        self.sync_channels();
        self.release_orphaned_voices();
    }

    fn retire_snapshot(&mut self) {
        if let Some(old) = self.snapshot.take()
            && let Err(rtrb::PushError::Full(old)) = self.garbage.push(old)
        {
            drop(old);
        }
    }

    /// Applies program, volume and pan for every track of the current
    /// snapshot without disturbing the voices that are already playing.
    fn sync_channels(&mut self) {
        let Some(snapshot) = self.snapshot.clone() else {
            return;
        };
        for track in &snapshot.tracks {
            let channel_index = usize::from(track.channel.min(15));
            let channel = channel_index as u32;
            let _ = self.synth.program_select(
                channel,
                self.font_id,
                u32::from(track.preset.bank),
                u32::from(track.preset.program),
            );
            self.channel_preset[channel_index] = Some(track.preset);
            let volume = (track.gain.clamp(0.0, 1.0) * 127.0).round() as u32;
            let pan = ((track.pan.clamp(-1.0, 1.0) + 1.0) * 63.5).round() as u32;
            let _ = self.synth.cc(channel, 7, volume);
            let _ = self.synth.cc(channel, 10, pan.min(127));
        }
    }

    fn load_soundfont(&mut self, path: std::path::PathBuf) {
        if let Ok(font_id) = self.synth.sfload(&path, true) {
            let previous = std::mem::replace(&mut self.font_id, font_id);
            let _ = self.synth.sfunload(previous, false);
            self.reset_synth();
        }
    }

    fn reset_synth(&mut self) {
        let _ = self.synth.system_reset();
        self.active = [0; CHANNELS];
        self.channel_preset = [None; CHANNELS];
        self.sync_channels();
    }

    fn render_preview(&mut self, left: &mut [f32], right: &mut [f32]) -> usize {
        if self.preview_frames == 0 && self.preview_sample.is_none() {
            left.fill(0.0);
            right.fill(0.0);
            self.peak_left = 0.0;
            self.peak_right = 0.0;
            self.publish();
            return 0;
        }

        let frames = if self.preview_frames > 0 {
            self.preview_frames.min(left.len())
        } else {
            0
        };
        if frames > 0 {
            let _ = self
                .synth
                .write((&mut left[..frames], &mut right[..frames]));
            self.preview_frames -= frames;
            if !self.preview_released && self.preview_frames <= self.preview_note_off_frames {
                self.preview_released = true;
                let _ = self.synth.note_off(self.preview_channel, self.preview_key);
            }
        }
        left[frames..].fill(0.0);
        right[frames..].fill(0.0);

        if let Some(mut voice) = self.preview_sample.take() {
            let step = f64::from(voice.data.sample_rate) / self.sample_rate;
            voice.mix_into(left, right, step);
            if !voice.finished() {
                self.preview_sample = Some(voice);
            }
        }

        self.peak_left = peak(left);
        self.peak_right = peak(right);
        self.publish();
        left.len()
    }

    fn start_sample_preview(&mut self, data: Arc<SampleData>) {
        if self.state == TransportState::Playing {
            return;
        }
        self.clear_preview();
        self.preview_sample = Some(SampleVoice::new(data, Ticks::ZERO, 1.0, 0.0));
    }

    fn preview_note(&mut self, note: PreviewNote) {
        if self.state == TransportState::Playing {
            return;
        }

        self.clear_preview();
        let channel = u32::from(note.channel.min(15));
        self.active = [0; CHANNELS];
        let _ = self.synth.system_reset();
        let _ = self.synth.program_select(
            channel,
            self.font_id,
            u32::from(note.preset.bank),
            u32::from(note.preset.program),
        );
        self.channel_preset[usize::from(note.channel.min(15))] = Some(note.preset);
        let volume = (note.gain.clamp(0.0, 1.0) * 127.0).round() as u32;
        let pan = ((note.pan.clamp(-1.0, 1.0) + 1.0) * 63.5).round() as u32;
        let _ = self.synth.cc(channel, 7, volume);
        let _ = self.synth.cc(channel, 10, pan.min(127));
        let _ = self
            .synth
            .note_on(channel, u32::from(note.key), u32::from(note.velocity));

        self.preview_frames = (self.sample_rate * PREVIEW_SECONDS).round() as usize;
        self.preview_note_off_frames =
            (self.sample_rate * (PREVIEW_SECONDS - PREVIEW_NOTE_OFF_SECONDS)).round() as usize;
        self.preview_channel = channel;
        self.preview_key = u32::from(note.key);
        self.preview_released = false;
    }

    fn clear_preview(&mut self) {
        self.preview_frames = 0;
        self.preview_note_off_frames = 0;
        self.preview_released = true;
        self.preview_sample = None;
    }

    fn trigger(&mut self, event: TimedEvent) {
        // Notes outside the MIDI range cannot be voiced; the model rejects them
        // but a hand-edited project file may still contain them.
        let Some(mask) = key_mask(event.kind.key()) else {
            return;
        };
        let channel_index = usize::from(event.channel.min(15));
        let channel = channel_index as u32;
        match event.kind {
            EventKind::NoteOn { key, velocity } => {
                // Patterns may name their own instrument; switch the channel
                // when it differs from what the track selected.
                if self.channel_preset[channel_index] != Some(event.preset) {
                    let _ = self.synth.program_select(
                        channel,
                        self.font_id,
                        u32::from(event.preset.bank),
                        u32::from(event.preset.program),
                    );
                    self.channel_preset[channel_index] = Some(event.preset);
                }
                let _ = self
                    .synth
                    .note_on(channel, u32::from(key), u32::from(velocity));
                self.active[channel_index] |= mask;
            }
            EventKind::NoteOff { key } => {
                let _ = self.synth.note_off(channel, u32::from(key));
                self.active[channel_index] &= !mask;
            }
        }
    }

    /// Releases voices whose note-off is no longer part of the schedule, e.g.
    /// because the note was deleted while it was sounding. Notes that still
    /// have a pending note-off are left alone so that an edit made during
    /// playback does not cut the current sound.
    fn release_orphaned_voices(&mut self) {
        let Some(snapshot) = self.snapshot.clone() else {
            return;
        };
        if self.active.iter().all(|mask| *mask == 0) {
            return;
        }

        let mut orphans = self.active;
        for event in &snapshot.events[self.event_index..] {
            if let EventKind::NoteOff { key } = event.kind
                && let Some(mask) = key_mask(key)
            {
                orphans[usize::from(event.channel.min(15))] &= !mask;
                if orphans.iter().all(|mask| *mask == 0) {
                    break;
                }
            }
        }

        for (channel, mask) in orphans.iter().enumerate() {
            let mut mask = *mask;
            while mask != 0 {
                let key = mask.trailing_zeros();
                mask &= mask - 1;
                let _ = self.synth.note_off(channel as u32, key);
            }
        }

        for (active, orphan) in self.active.iter_mut().zip(orphans) {
            *active &= !orphan;
        }
    }

    fn finish(&mut self, snapshot: &PlaybackSnapshot) {
        // The transport stops on the project boundary. Any event still pending
        // there - in practice the note-offs of notes ending on the last tick -
        // has to be delivered, otherwise those voices are left sounding.
        while let Some(event) = snapshot.events.get(self.event_index).copied() {
            self.trigger(event);
            self.event_index += 1;
        }
        self.sample_index = snapshot.samples.len();
        self.sample_voices.clear();
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

/// Bit of `key` inside a channel mask, or `None` for keys outside 0..=127.
const fn key_mask(key: u8) -> Option<u128> {
    if key < 128 { Some(1_u128 << key) } else { None }
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
        let pattern = project.add_pattern("Clip", Ticks(1920)).unwrap();
        project.add_clip(track, pattern, Ticks::ZERO).unwrap();
        project
            .add_note(pattern, 60, 110, Ticks::ZERO, Ticks(480))
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
    fn preview_renders_while_stopped_without_moving_the_transport() {
        let mut harness = harness();
        harness
            .commands
            .push(AudioCommand::PreviewNote(PreviewNote {
                channel: 0,
                preset: octava_core::Preset::default(),
                gain: 1.0,
                pan: 0.0,
                key: 60,
                velocity: 100,
            }))
            .unwrap();

        render(&mut harness.runtime, 2);
        assert_eq!(harness.telemetry.state(), TransportState::Stopped);
        assert_eq!(harness.telemetry.playhead(), Ticks::ZERO);
        let (left, right) = harness.telemetry.peaks();
        assert!(left > 0.0 || right > 0.0);

        render(&mut harness.runtime, 100);
        let (left, right) = harness.telemetry.peaks();
        assert_eq!(left, 0.0);
        assert_eq!(right, 0.0);
    }

    #[test]
    fn note_off_on_the_last_tick_is_delivered() {
        let mut project = Project::new("Boundary");
        let track = project.add_track("Piano").unwrap();
        let pattern = project.add_pattern("Clip", Ticks(1920)).unwrap();
        project.add_clip(track, pattern, Ticks::ZERO).unwrap();
        project
            .add_note(pattern, 60, 100, Ticks::ZERO, Ticks(1920))
            .unwrap();

        let mut harness = harness();
        harness
            .commands
            .push(AudioCommand::Play {
                snapshot: Arc::new(project.compile_snapshot()),
                start: Ticks::ZERO,
            })
            .unwrap();

        // 1920 ticks at 120 BPM are one second, i.e. ~94 blocks of 512 frames.
        render(&mut harness.runtime, 120);

        assert_eq!(harness.telemetry.state(), TransportState::Paused);
        assert!(
            harness.runtime.active.iter().all(|mask| *mask == 0),
            "a note ending on the last tick was never released"
        );
    }

    #[test]
    fn snapshot_update_keeps_sounding_notes_and_releases_deleted_ones() {
        let mut project = Project::new("Live edit");
        let track = project.add_track("Piano").unwrap();
        let pattern = project.add_pattern("Clip", Ticks(3840)).unwrap();
        project.add_clip(track, pattern, Ticks::ZERO).unwrap();
        project
            .add_note(pattern, 60, 100, Ticks::ZERO, Ticks(3840))
            .unwrap();

        let mut harness = harness();
        harness
            .commands
            .push(AudioCommand::Play {
                snapshot: Arc::new(project.compile_snapshot()),
                start: Ticks::ZERO,
            })
            .unwrap();
        render(&mut harness.runtime, 4);
        assert_eq!(harness.runtime.active[0] & (1 << 60), 1 << 60);

        // An edit that keeps the note: it must keep playing.
        harness
            .commands
            .push(AudioCommand::SetSnapshot(Arc::new(
                project.compile_snapshot(),
            )))
            .unwrap();
        render(&mut harness.runtime, 1);
        assert_eq!(
            harness.runtime.active[0] & (1 << 60),
            1 << 60,
            "the sounding note was cut by the snapshot update"
        );

        // An edit that deletes the note: the orphaned voice must be released.
        project.pattern_mut(pattern).unwrap().notes.clear();
        harness
            .commands
            .push(AudioCommand::SetSnapshot(Arc::new(
                project.compile_snapshot(),
            )))
            .unwrap();
        render(&mut harness.runtime, 1);
        assert_eq!(
            harness.runtime.active[0] & (1 << 60),
            0,
            "deleting a sounding note left its voice hanging"
        );
    }

    fn sample_snapshot(gain: f32, pan: f32) -> Arc<PlaybackSnapshot> {
        let data = Arc::new(SampleData {
            sample_rate: 48_000,
            channels: 1,
            frames: vec![0.5; 48_000],
        });
        Arc::new(PlaybackSnapshot {
            ppq: 960,
            tempo_map: octava_core::TempoMap::default(),
            length: Ticks(1920),
            tracks: Vec::new(),
            events: Vec::new(),
            samples: vec![octava_core::TimedSample {
                start: Ticks::ZERO,
                track: octava_core::TrackId(1),
                gain,
                pan,
                data,
            }],
        })
    }

    #[test]
    fn plays_audio_samples_with_the_transport() {
        let mut harness = harness();
        harness
            .commands
            .push(AudioCommand::Play {
                snapshot: sample_snapshot(1.0, 0.0),
                start: Ticks::ZERO,
            })
            .unwrap();

        render(&mut harness.runtime, 4);
        assert_eq!(harness.telemetry.state(), TransportState::Playing);
        let (left, right) = harness.telemetry.peaks();
        assert!(left > 0.1, "left peak was {left}");
        assert!(right > 0.1, "right peak was {right}");

        render(&mut harness.runtime, 120);
        assert_eq!(harness.telemetry.state(), TransportState::Paused);
        assert!(harness.runtime.sample_voices.is_empty());
    }

    #[test]
    fn sample_pan_moves_the_signal() {
        let mut harness = harness();
        harness
            .commands
            .push(AudioCommand::Play {
                snapshot: sample_snapshot(1.0, -1.0),
                start: Ticks::ZERO,
            })
            .unwrap();
        render(&mut harness.runtime, 4);
        let (left, right) = harness.telemetry.peaks();
        assert!(left > 0.1);
        assert!(right < 1.0e-4, "right peak was {right}");
    }

    #[test]
    fn previews_a_sample_while_stopped() {
        let mut harness = harness();
        let data = Arc::new(SampleData {
            sample_rate: 48_000,
            channels: 1,
            frames: vec![0.25; 48_000],
        });
        harness
            .commands
            .push(AudioCommand::PreviewSample(data))
            .unwrap();

        render(&mut harness.runtime, 2);
        assert_eq!(harness.telemetry.state(), TransportState::Stopped);
        assert_eq!(harness.telemetry.playhead(), Ticks::ZERO);
        let (left, right) = harness.telemetry.peaks();
        assert!(left > 0.1 || right > 0.1);

        render(&mut harness.runtime, 100);
        let (left, right) = harness.telemetry.peaks();
        assert_eq!(left, 0.0);
        assert_eq!(right, 0.0);
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
