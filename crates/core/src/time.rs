use std::fmt;
use std::ops::{Add, AddAssign};

use serde::{Deserialize, Serialize};

pub const DEFAULT_PPQ: u16 = 960;
pub const DEFAULT_BPM: f64 = 120.0;
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;

/// Musical time measured in ticks.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Ticks(pub u64);

impl Ticks {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    #[must_use]
    pub const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    #[must_use]
    pub const fn checked_sub(self, other: Self) -> Option<Self> {
        match self.0.checked_sub(other.0) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Converts a number of quarter notes to ticks.
    #[must_use]
    pub fn from_quarters(quarters: f64, ppq: u16) -> Self {
        Self((quarters * f64::from(ppq)).round().max(0.0) as u64)
    }
}

impl Add for Ticks {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for Ticks {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl From<u64> for Ticks {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl fmt::Display for Ticks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Time signature. A beat is `4 / denominator` quarter notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
        }
    }
}

impl TimeSignature {
    pub fn new(numerator: u8, denominator: u8) -> Result<Self, TimeError> {
        let signature = Self {
            numerator,
            denominator,
        };
        if signature.is_valid() {
            Ok(signature)
        } else {
            Err(TimeError::InvalidTimeSignature)
        }
    }

    #[must_use]
    pub fn is_valid(self) -> bool {
        self.numerator > 0
            && self.numerator <= 64
            && self.denominator.is_power_of_two()
            && self.denominator <= 64
    }

    #[must_use]
    pub fn ticks_per_beat(self, ppq: u16) -> u64 {
        (u64::from(ppq) * 4) / u64::from(self.denominator)
    }

    #[must_use]
    pub fn ticks_per_bar(self, ppq: u16) -> u64 {
        self.ticks_per_beat(ppq) * u64::from(self.numerator)
    }

    /// Returns `(bar, beat, tick_within_beat)`, all zero-based.
    #[must_use]
    pub fn bar_beat_tick(self, tick: Ticks, ppq: u16) -> (u64, u64, u64) {
        let bar_ticks = self.ticks_per_bar(ppq).max(1);
        let beat_ticks = self.ticks_per_beat(ppq).max(1);
        let bar = tick.0 / bar_ticks;
        let in_bar = tick.0 % bar_ticks;
        (bar, in_bar / beat_ticks, in_bar % beat_ticks)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TempoSegment {
    pub start: Ticks,
    pub bpm: f64,
}

impl TempoSegment {
    #[must_use]
    pub fn seconds_per_tick(&self, ppq: u16) -> f64 {
        seconds_per_tick(self.bpm, ppq)
    }
}

/// Piecewise-constant tempo map. The first segment always starts at tick zero.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TempoMap {
    ppq: u16,
    segments: Vec<TempoSegment>,
}

impl Default for TempoMap {
    fn default() -> Self {
        Self::new(DEFAULT_PPQ, DEFAULT_BPM).expect("default tempo map is valid")
    }
}

impl TempoMap {
    pub fn new(ppq: u16, bpm: f64) -> Result<Self, TimeError> {
        if ppq == 0 {
            return Err(TimeError::InvalidPpq);
        }
        validate_bpm(bpm)?;
        Ok(Self {
            ppq,
            segments: vec![TempoSegment {
                start: Ticks::ZERO,
                bpm,
            }],
        })
    }

    #[must_use]
    pub const fn ppq(&self) -> u16 {
        self.ppq
    }

    #[must_use]
    pub fn segments(&self) -> &[TempoSegment] {
        &self.segments
    }

    /// Adds a tempo change, replacing an existing segment at the same tick.
    pub fn insert_tempo(&mut self, at: Ticks, bpm: f64) -> Result<(), TimeError> {
        validate_bpm(bpm)?;
        match self
            .segments
            .binary_search_by_key(&at.0, |segment| segment.start.0)
        {
            Ok(index) => self.segments[index].bpm = bpm,
            Err(index) => self.segments.insert(index, TempoSegment { start: at, bpm }),
        }
        Ok(())
    }

    #[must_use]
    pub fn tempo_at(&self, tick: Ticks) -> f64 {
        let index = self
            .segments
            .partition_point(|segment| segment.start <= tick)
            .saturating_sub(1);
        self.segments[index].bpm
    }

    #[must_use]
    pub fn seconds_at(&self, tick: Ticks) -> f64 {
        let mut seconds = 0.0;
        let mut previous = &self.segments[0];

        for segment in self.segments.iter().skip(1) {
            if segment.start > tick {
                break;
            }
            seconds += tick_span_seconds(segment.start, previous.start, previous.bpm, self.ppq);
            previous = segment;
        }

        seconds + tick_span_seconds(tick, previous.start, previous.bpm, self.ppq)
    }

    /// Returns the fractional tick at the given absolute time.
    #[must_use]
    pub fn tick_at_seconds_f64(&self, seconds: f64) -> f64 {
        if seconds <= 0.0 {
            return 0.0;
        }

        let mut elapsed = 0.0;
        let mut previous = &self.segments[0];

        for segment in self.segments.iter().skip(1) {
            let segment_seconds =
                tick_span_seconds(segment.start, previous.start, previous.bpm, self.ppq);
            if seconds < elapsed + segment_seconds {
                break;
            }
            elapsed += segment_seconds;
            previous = segment;
        }

        let remaining = (seconds - elapsed).max(0.0);
        previous.start.0 as f64 + remaining / seconds_per_tick(previous.bpm, self.ppq)
    }

    #[must_use]
    pub fn tick_at_seconds(&self, seconds: f64) -> Ticks {
        Ticks(self.tick_at_seconds_f64(seconds).floor() as u64)
    }

    #[must_use]
    pub fn samples_at(&self, tick: Ticks, sample_rate: u32) -> u64 {
        (self.seconds_at(tick) * f64::from(sample_rate)).round() as u64
    }

    #[must_use]
    pub fn duration_seconds(&self, start: Ticks, end: Ticks) -> f64 {
        if end <= start {
            return 0.0;
        }
        self.seconds_at(end) - self.seconds_at(start)
    }
}

fn validate_bpm(bpm: f64) -> Result<(), TimeError> {
    if bpm.is_finite() && bpm > 0.0 {
        Ok(())
    } else {
        Err(TimeError::InvalidTempo)
    }
}

fn seconds_per_tick(bpm: f64, ppq: u16) -> f64 {
    60.0 / (bpm * f64::from(ppq))
}

fn tick_span_seconds(end: Ticks, start: Ticks, bpm: f64, ppq: u16) -> f64 {
    let ticks = end.0.saturating_sub(start.0);
    ticks as f64 * seconds_per_tick(bpm, ppq)
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TimeError {
    #[error("PPQ must be greater than zero")]
    InvalidPpq,
    #[error("tempo must be finite and greater than zero")]
    InvalidTempo,
    #[error(
        "time signature must have a numerator in 1..=64 and a power-of-two denominator in 1..=64"
    )]
    InvalidTimeSignature,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_note_conversion() {
        let tempo = TempoMap::default();

        assert!((tempo.seconds_at(Ticks(960)) - 0.5).abs() < 1e-9);
        assert!((tempo.seconds_at(Ticks(480)) - 0.25).abs() < 1e-9);
        assert_eq!(tempo.samples_at(Ticks(960), DEFAULT_SAMPLE_RATE), 24_000);
        assert_eq!(Ticks::from_quarters(2.5, 960), Ticks(2400));
    }

    #[test]
    fn tempo_changes_are_integrated() {
        let mut tempo = TempoMap::default();
        tempo.insert_tempo(Ticks(1920), 60.0).unwrap();

        assert!((tempo.seconds_at(Ticks(1920)) - 1.0).abs() < 1e-9);
        assert!((tempo.seconds_at(Ticks(3840)) - 3.0).abs() < 1e-9);
        assert_eq!(tempo.tick_at_seconds(2.5).0, 3360);
        assert_eq!(tempo.tempo_at(Ticks(1919)), 120.0);
        assert_eq!(tempo.tempo_at(Ticks(1920)), 60.0);
    }

    #[test]
    fn tick_at_seconds_inverts_seconds_at() {
        let mut tempo = TempoMap::default();
        tempo.insert_tempo(Ticks(1920), 90.0).unwrap();
        tempo.insert_tempo(Ticks(5000), 155.0).unwrap();

        for raw in [0, 17, 480, 1919, 1920, 2000, 4999, 5000, 7000, 12_345] {
            let tick = Ticks(raw);
            let seconds = tempo.seconds_at(tick);
            let round_trip = tempo.tick_at_seconds_f64(seconds);
            assert!(
                (round_trip - raw as f64).abs() < 1e-6,
                "tick {raw} round-tripped to {round_trip}"
            );
        }
    }

    #[test]
    fn tempo_map_rejects_invalid_values() {
        assert_eq!(TempoMap::new(0, 120.0), Err(TimeError::InvalidPpq));
        assert_eq!(TempoMap::new(960, 0.0), Err(TimeError::InvalidTempo));
        assert_eq!(TempoMap::new(960, f64::NAN), Err(TimeError::InvalidTempo));

        let mut tempo = TempoMap::default();
        assert_eq!(
            tempo.insert_tempo(Ticks(0), -1.0),
            Err(TimeError::InvalidTempo)
        );
    }

    #[test]
    fn time_signature_math() {
        let common = TimeSignature::default();
        assert_eq!(common.ticks_per_beat(960), 960);
        assert_eq!(common.ticks_per_bar(960), 3840);
        assert_eq!(common.bar_beat_tick(Ticks(4800), 960), (1, 1, 0));

        let six_eight = TimeSignature::new(6, 8).unwrap();
        assert_eq!(six_eight.ticks_per_beat(960), 480);
        assert_eq!(six_eight.ticks_per_bar(960), 2880);

        assert!(TimeSignature::new(0, 4).is_err());
        assert!(TimeSignature::new(4, 3).is_err());
    }
}
