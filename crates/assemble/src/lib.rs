//! Assemble a trimmed timeline from silence-detection keep regions.
//!
//! The silence detector (`hollywood-detect`) yields the spans of a source worth
//! keeping — the active speech — as keep regions over the media. This crate turns
//! those regions into a [`Timeline`]: each keep region becomes a clip of the
//! source, and the clips are laid back to back so the silent dead air between
//! them is removed. The assembled timeline is the rough cut an editor starts from
//! instead of a single untrimmed take.
//!
//! Keep regions are plain [`TimeRange`]s into the source, so this crate depends
//! only on the timeline IR, not on the detector that produced them — any spans
//! (a manual selection, a future VAD) assemble the same way. The result is
//! validated before it is returned, so callers receive a coherent timeline.

use hollywood_timeline::{
    Clip, FrameRate, MediaAsset, Seconds, TimeRange, Timeline, TimelineError, Track, TrackKind,
};
use thiserror::Error;

/// Failure assembling a timeline from keep regions.
#[derive(Debug, Error)]
pub enum AssembleError {
    /// Keep region `index` starts before the previous region ends. Keep regions
    /// must be ascending and non-overlapping, or the assembled cut would
    /// duplicate or reorder source content while still passing the IR's
    /// per-clip validation.
    #[error("keep region {index} is out of order or overlaps the previous region")]
    RegionsNotAscending {
        /// The position of the offending region in the input.
        index: usize,
    },

    /// Keep region `index` falls outside the source media — it starts before the
    /// source begins or reaches past its end.
    #[error("keep region {index} falls outside the source media")]
    RegionOutsideSource {
        /// The position of the offending region in the input.
        index: usize,
    },

    /// Keep region `index` has zero duration — there is nothing to keep.
    #[error("keep region {index} is empty")]
    EmptyRegion {
        /// The position of the offending region in the input.
        index: usize,
    },

    /// The assembled timeline failed the timeline IR's own validation. The
    /// checks above reject every bad-input case, so this is unreachable for
    /// caller input; it satisfies the `Result` of [`Timeline::validate`] and
    /// remains a safety net for an internal invariant violation.
    #[error("assembled timeline is invalid: {0}")]
    InvalidTimeline(#[from] TimelineError),
}

/// Assemble `keep_regions` of `asset` into a trimmed [`Timeline`] named `name` at
/// `frame_rate`.
///
/// Each keep region becomes a clip of the source in order, the clips laid end to
/// end so the dead air between regions is dropped — the assembled track runs only
/// as long as the kept spans, not the original source. The track is video when
/// the source carries a video stream, otherwise audio. An empty `keep_regions`
/// slice assembles an empty track.
///
/// Each region must be a positive-duration span within the source, and the
/// regions ascending and non-overlapping — the contract `hollywood-detect`
/// already satisfies. `frame_rate` is the timeline's own rate (the exporters'
/// frame grid); it is deliberately independent of a video source's native rate,
/// since mixed-rate timelines are ordinary and the kept spans are exact rational
/// time, not frame-snapped.
///
/// # Errors
///
/// Returns [`AssembleError::EmptyRegion`] for a zero-length region,
/// [`AssembleError::RegionOutsideSource`] if a region starts before the source or
/// reaches past its end, and [`AssembleError::RegionsNotAscending`] if a region
/// overlaps or precedes the previous one.
pub fn assemble(
    name: impl Into<String>,
    frame_rate: FrameRate,
    asset: MediaAsset,
    keep_regions: &[TimeRange],
) -> Result<Timeline, AssembleError> {
    let kind = track_kind(&asset);
    let source = asset.source().clone();
    let source_duration = asset.duration();

    // The IR validates each clip against the source but never compares clips to
    // one another, so overlapping or reversed regions would silently assemble
    // into a cut that duplicates or reorders content. Reject that input here,
    // where the regions are still regions.
    keep_regions
        .iter()
        .enumerate()
        .try_fold(Seconds::ZERO, |previous_end, (index, region)| {
            if region.duration().is_zero() {
                return Err(AssembleError::EmptyRegion { index });
            }
            if region.start().is_negative() || region.end() > source_duration {
                return Err(AssembleError::RegionOutsideSource { index });
            }
            // The region is within the source, so this catches only a genuine
            // ordering/overlap problem against the previous region (never a
            // negative first-region start, already rejected above).
            if region.start() < previous_end {
                return Err(AssembleError::RegionsNotAscending { index });
            }
            Ok(region.end())
        })?;

    let mut timeline = Timeline::new(name, frame_rate);
    timeline.add_asset(asset)?;

    let mut track = Track::new(kind);
    for region in keep_regions {
        track.push_clip(Clip::new(source.clone(), *region));
    }
    timeline.add_track(track);

    timeline.validate()?;
    Ok(timeline)
}

/// The track a source belongs on: video if it carries a video stream (its audio
/// rides with the clip), otherwise audio. This matches the IR's rule that a video
/// track's clips must reference a video stream.
fn track_kind(asset: &MediaAsset) -> TrackKind {
    if asset.video().is_some() {
        TrackKind::Video
    } else {
        TrackKind::Audio
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hollywood_timeline::{
        AudioProperties, ChannelLayout, MediaSource, SampleRate, Seconds, TrackItem,
        VideoProperties,
    };

    fn audio_asset(seconds: i64) -> MediaAsset {
        MediaAsset::new(
            MediaSource::file("vo.wav"),
            Seconds::from_secs(seconds),
            None,
            Some(AudioProperties {
                sample_rate: SampleRate::new(48_000).unwrap(),
                channels: ChannelLayout::Stereo,
            }),
        )
        .unwrap()
    }

    fn video_asset(seconds: i64) -> MediaAsset {
        MediaAsset::new(
            MediaSource::file("a.mov"),
            Seconds::from_secs(seconds),
            Some(VideoProperties {
                frame_rate: FrameRate::whole(30).unwrap(),
                width: 1920,
                height: 1080,
            }),
            None,
        )
        .unwrap()
    }

    fn region(start: i64, duration: i64) -> TimeRange {
        TimeRange::new(Seconds::from_secs(start), Seconds::from_secs(duration)).unwrap()
    }

    fn clip_ranges(timeline: &Timeline) -> Vec<TimeRange> {
        timeline.tracks()[0]
            .items()
            .iter()
            .filter_map(|item| match item {
                TrackItem::Clip(clip) => Some(clip.range()),
                TrackItem::Gap(_) | TrackItem::Transition(_) => None,
            })
            .collect()
    }

    #[test]
    fn removes_dead_air_between_keep_regions() {
        let fps = FrameRate::whole(30).unwrap();
        let timeline =
            assemble("cut", fps, audio_asset(10), &[region(0, 2), region(5, 3)]).unwrap();

        // The two kept spans become back-to-back clips; the [2, 5) silence is gone.
        assert_eq!(clip_ranges(&timeline), vec![region(0, 2), region(5, 3)]);
        // The assembled track is the sum of the kept spans, not the source length.
        assert_eq!(
            timeline.tracks()[0].occupied().unwrap(),
            Seconds::from_secs(5)
        );
    }

    #[test]
    fn empty_keep_regions_yield_an_empty_track() {
        let fps = FrameRate::whole(30).unwrap();
        let timeline = assemble("cut", fps, audio_asset(10), &[]).unwrap();

        assert!(clip_ranges(&timeline).is_empty());
        assert_eq!(timeline.tracks()[0].occupied().unwrap(), Seconds::ZERO);
    }

    #[test]
    fn video_source_assembles_onto_a_video_track() {
        let fps = FrameRate::whole(30).unwrap();
        let timeline = assemble("cut", fps, video_asset(10), &[region(0, 4)]).unwrap();
        assert_eq!(timeline.tracks()[0].kind(), TrackKind::Video);
    }

    #[test]
    fn audio_source_assembles_onto_an_audio_track() {
        let fps = FrameRate::whole(30).unwrap();
        let timeline = assemble("cut", fps, audio_asset(10), &[region(0, 4)]).unwrap();
        assert_eq!(timeline.tracks()[0].kind(), TrackKind::Audio);
    }

    #[test]
    fn keep_region_past_the_source_is_rejected() {
        let fps = FrameRate::whole(30).unwrap();
        // [8, 13) reaches past the 10 s source.
        let result = assemble("cut", fps, audio_asset(10), &[region(8, 5)]);
        assert!(matches!(
            result,
            Err(AssembleError::RegionOutsideSource { index: 0 })
        ));
    }

    #[test]
    fn keep_region_starting_before_the_source_is_rejected() {
        let fps = FrameRate::whole(30).unwrap();
        // A negative start lies before the source begins — outside it, not merely
        // out of order (there is no previous region to be out of order with).
        let result = assemble("cut", fps, audio_asset(10), &[region(-1, 3)]);
        assert!(matches!(
            result,
            Err(AssembleError::RegionOutsideSource { index: 0 })
        ));
    }

    #[test]
    fn adjacent_regions_are_kept_back_to_back() {
        let fps = FrameRate::whole(30).unwrap();
        // [0, 2) and [2, 3) touch exactly (no dead air between them): the strict
        // ordering check accepts a start equal to the previous region's end.
        let timeline =
            assemble("cut", fps, audio_asset(10), &[region(0, 2), region(2, 1)]).unwrap();
        assert_eq!(clip_ranges(&timeline), vec![region(0, 2), region(2, 1)]);
        assert_eq!(
            timeline.tracks()[0].occupied().unwrap(),
            Seconds::from_secs(3)
        );
    }

    #[test]
    fn region_ending_exactly_at_the_source_end_is_accepted() {
        let fps = FrameRate::whole(30).unwrap();
        // [7, 10) ends exactly at the 10 s source: the bound is half-open, so this
        // last-frame span is kept, not rejected.
        let timeline = assemble("cut", fps, audio_asset(10), &[region(7, 3)]).unwrap();
        assert_eq!(clip_ranges(&timeline), vec![region(7, 3)]);
    }

    #[test]
    fn overlapping_regions_are_rejected() {
        let fps = FrameRate::whole(30).unwrap();
        // [3, 6) overlaps the preceding [0, 5): the cut would duplicate [3, 5).
        let result = assemble("cut", fps, audio_asset(10), &[region(0, 5), region(3, 3)]);
        assert!(matches!(
            result,
            Err(AssembleError::RegionsNotAscending { index: 1 })
        ));
    }

    #[test]
    fn out_of_order_regions_are_rejected() {
        let fps = FrameRate::whole(30).unwrap();
        // The second region precedes the first in the source.
        let result = assemble("cut", fps, audio_asset(10), &[region(5, 3), region(0, 2)]);
        assert!(matches!(
            result,
            Err(AssembleError::RegionsNotAscending { index: 1 })
        ));
    }

    #[test]
    fn empty_region_is_rejected() {
        let fps = FrameRate::whole(30).unwrap();
        let result = assemble("cut", fps, audio_asset(10), &[region(2, 0)]);
        assert!(matches!(
            result,
            Err(AssembleError::EmptyRegion { index: 0 })
        ));
    }
}
