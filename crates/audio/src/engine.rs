use std::path::Path;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use fluidlite::{FontId, IsSettings as _, Settings, Synth};
use octava_core::{PlaybackSnapshot, Preset, Ticks};
use rtrb::{Consumer, Producer, RingBuffer};
use thiserror::Error;

use crate::runtime::Runtime;
use crate::soundfont;

const COMMAND_CAPACITY: usize = 128;
const GARBAGE_CAPACITY: usize = 16;
const RENDER_BLOCK_FRAMES: usize = 1024;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("no audio output device is available")]
    NoOutputDevice,
    #[error("audio device error: {0}")]
    Device(String),
    #[error("synthesizer error: {0}")]
    Synth(String),
    #[error("sound font '{path}' could not be loaded: {source}")]
    SoundFont {
        path: String,
        #[source]
        source: fluidlite::Error,
    },
    #[error("unsupported output sample format: {0:?}")]
    UnsupportedSampleFormat(cpal::SampleFormat),
    #[error("audio command queue is full")]
    CommandQueueFull,
    #[error("sample rate must be greater than zero")]
    InvalidSampleRate,
    #[error("tail length must be a finite, non-negative number of seconds")]
    InvalidTailSeconds,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum TransportState {
    #[default]
    Stopped = 0,
    Playing = 1,
    Paused = 2,
}

impl TransportState {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Playing,
            2 => Self::Paused,
            _ => Self::Stopped,
        }
    }
}

pub struct AudioTelemetry {
    state: std::sync::atomic::AtomicU8,
    playhead: std::sync::atomic::AtomicU64,
    peak_left: std::sync::atomic::AtomicU32,
    peak_right: std::sync::atomic::AtomicU32,
    sample_rate: u32,
}

impl AudioTelemetry {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            state: std::sync::atomic::AtomicU8::new(TransportState::Stopped as u8),
            playhead: std::sync::atomic::AtomicU64::new(0),
            peak_left: std::sync::atomic::AtomicU32::new(0.0_f32.to_bits()),
            peak_right: std::sync::atomic::AtomicU32::new(0.0_f32.to_bits()),
            sample_rate,
        }
    }

    #[must_use]
    pub fn state(&self) -> TransportState {
        TransportState::from_u8(self.state.load(std::sync::atomic::Ordering::Acquire))
    }

    #[must_use]
    pub fn playhead(&self) -> Ticks {
        Ticks(self.playhead.load(std::sync::atomic::Ordering::Relaxed))
    }

    #[must_use]
    pub fn peaks(&self) -> (f32, f32) {
        (
            f32::from_bits(self.peak_left.load(std::sync::atomic::Ordering::Relaxed)),
            f32::from_bits(self.peak_right.load(std::sync::atomic::Ordering::Relaxed)),
        )
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) fn publish(
        &self,
        state: TransportState,
        playhead: Ticks,
        peak_left: f32,
        peak_right: f32,
    ) {
        self.state
            .store(state as u8, std::sync::atomic::Ordering::Release);
        self.playhead
            .store(playhead.0, std::sync::atomic::Ordering::Relaxed);
        self.peak_left
            .store(peak_left.to_bits(), std::sync::atomic::Ordering::Relaxed);
        self.peak_right
            .store(peak_right.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewNote {
    pub channel: u8,
    pub preset: Preset,
    pub gain: f32,
    pub pan: f32,
    pub key: u8,
    pub velocity: u8,
}

pub(crate) enum AudioCommand {
    Play {
        snapshot: Arc<PlaybackSnapshot>,
        start: Ticks,
    },
    Resume,
    Pause,
    Stop,
    Seek(Ticks),
    SetSnapshot(Arc<PlaybackSnapshot>),
    PreviewNote(PreviewNote),
}

pub struct AudioEngine {
    _stream: cpal::Stream,
    commands: Producer<AudioCommand>,
    garbage: Consumer<Arc<PlaybackSnapshot>>,
    telemetry: Arc<AudioTelemetry>,
    device_name: String,
}

impl AudioEngine {
    pub fn start() -> Result<Self, AudioError> {
        Self::start_with_soundfont(soundfont::default_path())
    }

    pub fn start_with_soundfont(path: impl AsRef<Path>) -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(AudioError::NoOutputDevice)?;
        let device_name = device.description().map_or_else(
            |_| "default output".to_owned(),
            |description| description.name().to_owned(),
        );
        let supported = device
            .default_output_config()
            .map_err(|error| AudioError::Device(error.to_string()))?;
        let sample_format = supported.sample_format();
        let config = supported.config();
        let sample_rate = config.sample_rate;

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
            Arc::clone(&telemetry),
        );

        let stream = match sample_format {
            cpal::SampleFormat::I8 => build_stream::<i8>(&device, config, runtime),
            cpal::SampleFormat::I16 => build_stream::<i16>(&device, config, runtime),
            cpal::SampleFormat::I32 => build_stream::<i32>(&device, config, runtime),
            cpal::SampleFormat::I64 => build_stream::<i64>(&device, config, runtime),
            cpal::SampleFormat::U8 => build_stream::<u8>(&device, config, runtime),
            cpal::SampleFormat::U16 => build_stream::<u16>(&device, config, runtime),
            cpal::SampleFormat::U32 => build_stream::<u32>(&device, config, runtime),
            cpal::SampleFormat::U64 => build_stream::<u64>(&device, config, runtime),
            cpal::SampleFormat::F32 => build_stream::<f32>(&device, config, runtime),
            cpal::SampleFormat::F64 => build_stream::<f64>(&device, config, runtime),
            other => Err(AudioError::UnsupportedSampleFormat(other)),
        }?;
        stream
            .play()
            .map_err(|error| AudioError::Device(error.to_string()))?;

        Ok(Self {
            _stream: stream,
            commands,
            garbage: garbage_consumer,
            telemetry,
            device_name,
        })
    }

    #[must_use]
    pub fn telemetry(&self) -> &AudioTelemetry {
        &self.telemetry
    }

    #[must_use]
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.telemetry.sample_rate()
    }

    pub fn poll(&mut self) {
        while self.garbage.pop().is_ok() {}
    }

    pub fn play(
        &mut self,
        snapshot: Arc<PlaybackSnapshot>,
        start: Ticks,
    ) -> Result<(), AudioError> {
        self.send(AudioCommand::Play { snapshot, start })
    }

    pub fn resume(&mut self) -> Result<(), AudioError> {
        self.send(AudioCommand::Resume)
    }

    pub fn pause(&mut self) -> Result<(), AudioError> {
        self.send(AudioCommand::Pause)
    }

    pub fn stop(&mut self) -> Result<(), AudioError> {
        self.send(AudioCommand::Stop)
    }

    pub fn seek(&mut self, tick: Ticks) -> Result<(), AudioError> {
        self.send(AudioCommand::Seek(tick))
    }

    pub fn set_snapshot(&mut self, snapshot: Arc<PlaybackSnapshot>) -> Result<(), AudioError> {
        self.send(AudioCommand::SetSnapshot(snapshot))
    }

    pub fn preview_note(&mut self, note: PreviewNote) -> Result<(), AudioError> {
        self.send(AudioCommand::PreviewNote(note))
    }

    fn send(&mut self, command: AudioCommand) -> Result<(), AudioError> {
        self.commands
            .push(command)
            .map_err(|_| AudioError::CommandQueueFull)
    }
}

pub(crate) fn create_synth(path: &Path, sample_rate: u32) -> Result<(Synth, FontId), AudioError> {
    let settings = Settings::new().map_err(|error| AudioError::Synth(error.to_string()))?;
    // FluidLite mixes the chorus into both output channels, which drowns the
    // per-voice pan and makes the mixer's balance control inaudible.
    if let Some(chorus) = settings.str_("synth.chorus.active") {
        chorus.set("no");
    }
    let synth = Synth::new(settings).map_err(|error| AudioError::Synth(error.to_string()))?;
    synth.set_sample_rate(sample_rate as f32);
    let _ = synth.set_polyphony(128);
    synth.set_gain(0.75);
    let prepared = soundfont::prepare(path);
    let font_id = synth
        .sfload(&prepared, true)
        .map_err(|source| AudioError::SoundFont {
            path: path.display().to_string(),
            source,
        })?;
    Ok((synth, font_id))
}

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut runtime: Runtime,
) -> Result<cpal::Stream, AudioError>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = usize::from(config.channels);
    if channels == 0 {
        return Err(AudioError::Device(
            "output device reported zero channels".to_owned(),
        ));
    }

    let mut scratch_left = vec![0.0_f32; RENDER_BLOCK_FRAMES];
    let mut scratch_right = vec![0.0_f32; RENDER_BLOCK_FRAMES];

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _info| {
                let frames = data.len() / channels;
                let mut written = 0;
                while written < frames {
                    let block = (frames - written).min(RENDER_BLOCK_FRAMES);
                    let left = &mut scratch_left[..block];
                    let right = &mut scratch_right[..block];
                    runtime.render(left, right);

                    let output = &mut data[written * channels..(written + block) * channels];
                    for (frame, (left_sample, right_sample)) in output
                        .chunks_exact_mut(channels)
                        .zip(left.iter().zip(right))
                    {
                        frame[0] = T::from_sample(*left_sample);
                        if channels == 1 {
                            frame[0] = T::from_sample((*left_sample + *right_sample) * 0.5);
                        } else {
                            frame[1] = T::from_sample(*right_sample);
                            for sample in &mut frame[2..] {
                                *sample = T::from_sample(0.0_f32);
                            }
                        }
                    }

                    written += block;
                }

                // `data` is normally an exact number of frames; zero whatever
                // is left so that a device cannot pick up stale garbage.
                for sample in &mut data[frames * channels..] {
                    *sample = T::from_sample(0.0_f32);
                }
            },
            move |error| eprintln!("octava audio stream error: {error}"),
            None,
        )
        .map_err(|error| AudioError::Device(error.to_string()))
}
