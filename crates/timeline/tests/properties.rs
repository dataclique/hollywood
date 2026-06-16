//! Property-based tests for the timeline IR invariants.

use hollywood_timeline::{
    Clip, FrameRate, Gap, MediaAsset, MediaSource, SampleRate, Seconds, TimeRange, Timeline,
    TimelineError, Track, TrackKind, Transition, VideoProperties,
};
use proptest::prelude::*;

// These strategy helpers are not `#[test]` functions, so the workspace's
// allow-unwrap/expect-in-tests does not cover them. `prop_filter_map` with
// `.ok()` is the lint-clean way to build a strategy from a fallible
// constructor — the denominator range is always >= 1, so the filter never
// actually discards a value.

/// Any rational seconds with a bounded numerator/denominator (kept small enough
/// that the exercised arithmetic stays well inside `i64`).
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

/// Strictly-positive rational seconds.
fn positive_seconds() -> impl Strategy<Value = Seconds> {
    (1i64..1_000_000, 1i64..100_000).prop_filter_map("non-zero denominator", |(num, den)| {
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
        prop_assert_eq!(
            TimeRange::new(start, negative),
            Err(TimelineError::NegativeDuration)
        );
    }

    #[test]
    fn from_frames_is_monotonic(a in 0i64..1_000_000, b in 0i64..1_000_000) {
        let rate = FrameRate::new(30_000, 1001).unwrap();
        prop_assert_eq!(a <= b, Seconds::from_frames(a, rate) <= Seconds::from_frames(b, rate));
    }

    #[test]
    fn contains_is_inclusive_of_start_exclusive_of_end(
        start in non_negative_seconds(),
        duration in positive_seconds(),
    ) {
        let range = TimeRange::new(start, duration).unwrap();
        // The half-open contract: start is in, end is out.
        prop_assert!(range.contains(range.start()));
        prop_assert!(!range.contains(range.end()));
    }

    #[test]
    fn points_outside_the_range_are_never_contained(
        start in non_negative_seconds(),
        duration in positive_seconds(),
        delta in positive_seconds(),
    ) {
        let range = TimeRange::new(start, duration).unwrap();
        // A point strictly before start is never contained.
        if let Some(before) = start.checked_sub(delta) {
            prop_assert!(!range.contains(before));
        }
        // A point at or after end is never contained.
        if let Some(after) = range.end().checked_add(delta) {
            prop_assert!(!range.contains(after));
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

    #[test]
    fn overlaps_is_reflexive_symmetric_and_excludes_adjacent(
        start in non_negative_seconds(),
        first in positive_seconds(),
        second in positive_seconds(),
    ) {
        let a = TimeRange::new(start, first).unwrap();
        // Reflexive: a non-empty range always overlaps itself.
        prop_assert!(a.overlaps(a));
        // Adjacent ranges meeting at a single instant never overlap (half-open).
        let adjacent = TimeRange::new(a.end(), second).unwrap();
        prop_assert!(!a.overlaps(adjacent));
        prop_assert!(!adjacent.overlaps(a));
        // Symmetric: two ranges sharing a start overlap, in both directions.
        let same_start = TimeRange::new(start, second).unwrap();
        prop_assert!(a.overlaps(same_start));
        prop_assert_eq!(a.overlaps(same_start), same_start.overlaps(a));
    }
}

proptest! {
    /// A clip-bounded `[clip, transition, clip]` sequence is always
    /// structurally valid, whatever the (positive) durations.
    #[test]
    fn clip_transition_clip_always_validates(
        first in positive_seconds(),
        fade in positive_seconds(),
        second in positive_seconds(),
    ) {
        // A cross-fade cannot exceed either clip it overlaps.
        let fade = fade.min(first).min(second);
        let source = MediaSource::file("a.mov");
        let mut track = Track::new(TrackKind::Audio);
        track.push_clip(Clip::new(source.clone(), TimeRange::from_origin(first).unwrap()));
        track
            .push_transition(Transition::cross_fade(fade).unwrap())
            .unwrap();
        track.push_clip(Clip::new(source, TimeRange::from_origin(second).unwrap()));
        prop_assert_eq!(track.validate(), Ok(()));
    }

    /// A track alternating clips and transitions
    /// (`[clip, transition, clip, transition, clip]`) is always structurally
    /// valid — every transition is flanked by clips, including the interior one.
    #[test]
    fn alternating_clips_and_transitions_validate(
        first in positive_seconds(),
        fade_a in positive_seconds(),
        second in positive_seconds(),
        fade_b in positive_seconds(),
        third in positive_seconds(),
    ) {
        // Each cross-fade is bounded by the two clips it overlaps.
        let fade_a = fade_a.min(first).min(second);
        let fade_b = fade_b.min(second).min(third);
        let source = MediaSource::file("a.mov");
        let mut track = Track::new(TrackKind::Audio);
        track.push_clip(Clip::new(source.clone(), TimeRange::from_origin(first).unwrap()));
        track
            .push_transition(Transition::cross_fade(fade_a).unwrap())
            .unwrap();
        track.push_clip(Clip::new(source.clone(), TimeRange::from_origin(second).unwrap()));
        track
            .push_transition(Transition::cross_fade(fade_b).unwrap())
            .unwrap();
        track.push_clip(Clip::new(source, TimeRange::from_origin(third).unwrap()));
        prop_assert_eq!(track.validate(), Ok(()));
    }

    /// `Timeline::validate` never accepts a clip whose source range runs past
    /// the end of its asset.
    #[test]
    fn clip_exceeding_its_asset_never_validates(
        asset_duration in positive_seconds(),
        overshoot in positive_seconds(),
    ) {
        let source = MediaSource::file("a.mov");
        let video = VideoProperties {
            frame_rate: FrameRate::whole(30).unwrap(),
            width: 1920,
            height: 1080,
        };
        let mut timeline = Timeline::new("t", FrameRate::whole(30).unwrap());
        timeline
            .add_asset(MediaAsset::new(source.clone(), asset_duration, Some(video), None).unwrap())
            .unwrap();
        prop_assume!(asset_duration.checked_add(overshoot).is_some());
        let clip_span = asset_duration
            .checked_add(overshoot)
            .expect("sum is representable per prop_assume above");
        let mut track = Track::new(TrackKind::Video);
        track.push_clip(Clip::new(source, TimeRange::from_origin(clip_span).unwrap()));
        timeline.add_track(track);
        prop_assert_eq!(timeline.validate(), Err(TimelineError::ClipOutOfAssetBounds));
    }

    /// Gaps and transitions reject any non-positive duration.
    #[test]
    fn gaps_and_transitions_reject_non_positive(
        num in -1_000_000i64..=0,
        den in 1i64..100_000,
    ) {
        let duration = Seconds::new(num, den).unwrap();
        prop_assert!(Gap::new(duration).is_err());
        prop_assert!(Transition::cross_fade(duration).is_err());
    }
}
