//! Property-based tests for the timeline IR invariants.

use hollywood_timeline::{FrameRate, SampleRate, Seconds, TimeRange};
use proptest::prelude::*;

/// Any rational seconds with a bounded numerator/denominator (kept small enough
/// that the exercised arithmetic stays well inside `i64`). The denominator
/// range is non-zero, so the `.ok()` never filters anything out.
fn any_seconds() -> impl Strategy<Value = Seconds> {
    (-1_000_000i64..1_000_000, 1i64..100_000)
        .prop_filter_map("non-zero denominator", |(num, den)| {
            Seconds::new(num, den).ok()
        })
}

/// Non-negative rational seconds.
fn non_negative_seconds() -> impl Strategy<Value = Seconds> {
    (0i64..1_000_000, 1i64..100_000).prop_filter_map("non-zero denominator", |(num, den)| {
        Seconds::new(num, den).ok()
    })
}

proptest! {
    #[test]
    fn time_range_end_never_precedes_start(start in any_seconds(), duration in non_negative_seconds()) {
        let range = TimeRange::new(start, duration).unwrap();
        prop_assert!(range.end() >= range.start());
    }

    #[test]
    fn negative_duration_is_always_rejected(start in any_seconds(), num in -1_000_000i64..-1, den in 1i64..100_000) {
        let negative = Seconds::new(num, den).unwrap();
        prop_assert!(TimeRange::new(start, negative).is_err());
    }

    #[test]
    fn from_frames_is_monotonic(a in 0i64..1_000_000, b in 0i64..1_000_000) {
        let rate = FrameRate::new(30_000, 1001).unwrap();
        prop_assert_eq!(a <= b, Seconds::from_frames(a, rate) <= Seconds::from_frames(b, rate));
    }

    #[test]
    fn contains_implies_within_half_open_bounds(
        start in non_negative_seconds(),
        duration in non_negative_seconds(),
        probe in non_negative_seconds(),
    ) {
        let range = TimeRange::new(start, duration).unwrap();
        if range.contains(probe) {
            prop_assert!(probe >= range.start());
            prop_assert!(probe < range.end());
        }
    }

    #[test]
    fn whole_seconds_of_samples_round_trip(
        rate in prop_oneof![Just(44_100u32), Just(48_000u32), Just(96_000u32)],
        seconds in 0i64..3600,
    ) {
        let sample_rate = SampleRate::new(rate).unwrap();
        let from_samples = Seconds::from_samples(seconds * i64::from(rate), sample_rate);
        prop_assert_eq!(from_samples, Seconds::from_secs(seconds));
    }
}
