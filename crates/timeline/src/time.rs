//! Exact rational time and the rate newtypes the timeline measures against.
//!
//! Time is stored as an exact `i64/i64` rational number of seconds, never
//! floating-point, so conversions between frame rates and sample rates never
//! drift.

use std::num::NonZeroU32;
use std::ops::{Add, AddAssign, Sub, SubAssign};

use num_rational::Rational64;
use num_traits::{CheckedAdd, CheckedSub, Signed, ToPrimitive, Zero};

use crate::error::TimelineError;

/// An exact instant or duration, in seconds.
///
/// Stored as an `i64/i64` rational, so conversions between frame and sample
/// rates never drift. The `+`/`-` operators and [`from_frames`](Self::from_frames)
/// follow the standard overflow convention — they may panic only at magnitudes
/// far beyond any realistic media duration. For untrusted inputs use
/// [`checked_add`](Self::checked_add) / [`checked_sub`](Self::checked_sub).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seconds(Rational64);

impl Seconds {
    /// Zero seconds.
    pub const ZERO: Self = Self(Rational64::new_raw(0, 1));

    /// `numerator / denominator` seconds. Errors on a zero denominator, and on
    /// a negative denominator — write the sign on the numerator instead, so
    /// `Seconds::new(1, -1)` is rejected rather than silently read as `-1`.
    pub fn new(numerator: i64, denominator: i64) -> Result<Self, TimelineError> {
        if denominator == 0 {
            return Err(TimelineError::ZeroDenominator);
        }
        if denominator < 0 {
            return Err(TimelineError::NegativeDenominator);
        }
        Ok(Self(Rational64::new(numerator, denominator)))
    }

    /// A whole number of seconds.
    pub fn from_secs(seconds: i64) -> Self {
        Self(Rational64::from_integer(seconds))
    }

    /// The duration spanned by `frames` whole frames at `rate`.
    pub fn from_frames(frames: i64, rate: FrameRate) -> Self {
        Self(Rational64::from_integer(frames) / rate.0)
    }

    /// The duration spanned by `samples` samples at `rate`.
    pub fn from_samples(samples: i64, rate: SampleRate) -> Self {
        Self(Rational64::new(samples, i64::from(rate.0.get())))
    }

    /// The number of whole samples this duration spans at `rate`, rounded to the
    /// nearest sample. Lossy for a sub-sample duration; the exact inverse holds
    /// only when the duration is already a whole number of samples at `rate`.
    pub fn to_samples(self, rate: SampleRate) -> i64 {
        (self.0 * Rational64::from_integer(i64::from(rate.0.get())))
            .round()
            .to_integer()
    }

    /// The number of whole frames this duration spans at `rate`, rounded to the
    /// nearest frame. This is lossy for a sub-frame duration; to test exact
    /// frame alignment use the round-trip guard `from_frames(to_frames(t)) == t`
    /// rather than treating this as the inverse of
    /// [`from_frames`](Self::from_frames).
    pub fn to_frames(self, rate: FrameRate) -> i64 {
        (self.0 * rate.0).round().to_integer()
    }

    /// Whether this value is strictly less than zero.
    pub fn is_negative(self) -> bool {
        self.0.is_negative()
    }

    /// Whether this value is exactly zero.
    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    /// This value as an `f64` number of seconds, for display and FFI.
    pub fn as_secs_f64(self) -> f64 {
        self.0.to_f64().unwrap_or(f64::NAN)
    }

    /// `self + rhs`, returning `None` on `i64` overflow instead of panicking.
    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.0.checked_add(&rhs.0).map(Self)
    }

    /// `self - rhs`, returning `None` on `i64` overflow instead of panicking.
    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        self.0.checked_sub(&rhs.0).map(Self)
    }
}

impl Add for Seconds {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Seconds {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl AddAssign for Seconds {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl SubAssign for Seconds {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

/// Frames per second, an exact rational (e.g. `30000/1001` for 29.97 fps).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameRate(Rational64);

impl FrameRate {
    /// `numerator / denominator` frames per second. Both must be positive.
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, TimelineError> {
        if numerator == 0 || denominator == 0 {
            return Err(TimelineError::NonPositiveFrameRate);
        }
        Ok(Self(Rational64::new(
            i64::from(numerator),
            i64::from(denominator),
        )))
    }

    /// A whole-number frame rate, e.g. `FrameRate::whole(30)`.
    pub fn whole(fps: u32) -> Result<Self, TimelineError> {
        Self::new(fps, 1)
    }

    /// This rate as an `f64` (frames per second).
    pub fn as_f64(self) -> f64 {
        self.0.to_f64().unwrap_or(f64::NAN)
    }

    /// The whole frames per second, if this rate is an integer — i.e. not a
    /// fractional NTSC rate like `30000/1001`.
    pub fn as_whole(self) -> Option<u32> {
        if *self.0.denom() == 1 {
            u32::try_from(*self.0.numer()).ok()
        } else {
            None
        }
    }
}

/// Audio samples per second (e.g. 48 000 Hz).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SampleRate(NonZeroU32);

impl SampleRate {
    /// A sample rate in hertz. Must be non-zero.
    pub fn new(hertz: u32) -> Result<Self, TimelineError> {
        NonZeroU32::new(hertz)
            .map(Self)
            .ok_or(TimelineError::ZeroSampleRate)
    }

    /// The rate in hertz.
    pub fn hertz(self) -> u32 {
        self.0.get()
    }
}

/// A half-open span `[start, start + duration)` in exact seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeRange {
    start: Seconds,
    duration: Seconds,
}

impl TimeRange {
    /// A range starting at `start` lasting `duration`. Errors if `duration` is
    /// negative, or if `start + duration` is not representable in exact `i64`
    /// rational seconds — so [`end`](Self::end) can never overflow.
    pub fn new(start: Seconds, duration: Seconds) -> Result<Self, TimelineError> {
        if duration.is_negative() {
            return Err(TimelineError::NegativeDuration);
        }
        if start.checked_add(duration).is_none() {
            return Err(TimelineError::TimeRangeOverflow);
        }
        Ok(Self { start, duration })
    }

    /// A range from the timeline origin lasting `duration`.
    pub fn from_origin(duration: Seconds) -> Result<Self, TimelineError> {
        Self::new(Seconds::ZERO, duration)
    }

    /// The start instant.
    pub fn start(self) -> Seconds {
        self.start
    }

    /// The span's length.
    pub fn duration(self) -> Seconds {
        self.duration
    }

    /// The (exclusive) end instant, `start + duration`. Cannot overflow:
    /// [`new`](Self::new) rejects any range whose end is not representable.
    pub fn end(self) -> Seconds {
        self.start + self.duration
    }

    /// Whether `at` lies in `[start, end)`.
    pub fn contains(self, at: Seconds) -> bool {
        self.start <= at && at < self.end()
    }

    /// Whether the two ranges share any instant.
    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end() && other.start < self.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_convert_to_exact_seconds() {
        let rate = FrameRate::new(30000, 1001).unwrap();
        // 30000 frames at 29.97 fps is exactly 1001 seconds, with no drift.
        assert_eq!(Seconds::from_frames(30000, rate), Seconds::from_secs(1001));
    }

    #[test]
    fn seconds_round_trip_through_frames() {
        let rate = FrameRate::whole(24).unwrap();
        assert_eq!(Seconds::from_secs(2).to_frames(rate), 48);
        assert_eq!(Seconds::from_frames(48, rate).to_frames(rate), 48);
    }

    #[test]
    fn samples_convert_to_exact_seconds() {
        let rate = SampleRate::new(48_000).unwrap();
        assert_eq!(Seconds::from_samples(48_000, rate), Seconds::from_secs(1));
        assert_eq!(
            Seconds::from_samples(24_000, rate),
            Seconds::new(1, 2).unwrap()
        );
    }

    #[test]
    fn seconds_convert_to_samples() {
        let rate = SampleRate::new(48_000).unwrap();
        assert_eq!(Seconds::from_secs(1).to_samples(rate), 48_000);
        assert_eq!(Seconds::new(1, 2).unwrap().to_samples(rate), 24_000);
        // Sub-sample durations round to the nearest sample (1.2 samples -> 1).
        assert_eq!(Seconds::new(1, 40_000).unwrap().to_samples(rate), 1);
    }

    #[test]
    fn zero_denominator_is_rejected() {
        assert_eq!(Seconds::new(1, 0), Err(TimelineError::ZeroDenominator));
    }

    #[test]
    fn negative_denominator_is_rejected() {
        assert_eq!(Seconds::new(1, -1), Err(TimelineError::NegativeDenominator));
    }

    #[test]
    fn rates_reject_zero() {
        assert_eq!(
            FrameRate::new(0, 1),
            Err(TimelineError::NonPositiveFrameRate)
        );
        assert_eq!(
            FrameRate::new(30, 0),
            Err(TimelineError::NonPositiveFrameRate)
        );
        assert_eq!(SampleRate::new(0), Err(TimelineError::ZeroSampleRate));
    }

    #[test]
    fn arithmetic_and_ordering() {
        let one = Seconds::from_secs(1);
        let half = Seconds::new(1, 2).unwrap();
        assert_eq!(one + half, Seconds::new(3, 2).unwrap());
        assert_eq!(one - half, half);
        assert!((one - one).is_zero());
        assert!((half - one).is_negative());
        assert!(half < one);
    }

    #[test]
    fn checked_arithmetic_guards_overflow() {
        assert_eq!(
            Seconds::from_secs(2).checked_add(Seconds::from_secs(3)),
            Some(Seconds::from_secs(5))
        );
        let huge = Seconds::new(i64::MAX, 1).unwrap();
        assert_eq!(huge.checked_add(Seconds::from_secs(1)), None);
    }

    #[test]
    fn time_range_rejects_negative_duration() {
        let neg = Seconds::new(-1, 1).unwrap();
        assert_eq!(
            TimeRange::new(Seconds::ZERO, neg),
            Err(TimelineError::NegativeDuration)
        );
    }

    #[test]
    fn time_range_rejects_unrepresentable_end() {
        // start + duration overflows i64, so end() would panic — reject at
        // construction instead.
        let near_max = Seconds::new(i64::MAX, 1).unwrap();
        let one = Seconds::from_secs(1);
        assert_eq!(
            TimeRange::new(near_max, one),
            Err(TimelineError::TimeRangeOverflow)
        );
    }

    #[test]
    fn time_range_contains_and_overlaps() {
        let a = TimeRange::new(Seconds::from_secs(0), Seconds::from_secs(10)).unwrap();
        let b = TimeRange::new(Seconds::from_secs(5), Seconds::from_secs(10)).unwrap();
        let c = TimeRange::new(Seconds::from_secs(10), Seconds::from_secs(5)).unwrap();
        assert_eq!(a.end(), Seconds::from_secs(10));
        assert!(a.contains(Seconds::from_secs(0)));
        assert!(!a.contains(Seconds::from_secs(10)));
        assert!(a.overlaps(b));
        assert!(!a.overlaps(c));
    }
}
