//! The timeline: a registry of media assets and the tracks that reference them.

use std::collections::HashMap;

use crate::asset::{MediaAsset, MediaSource};
use crate::error::TimelineError;
use crate::time::FrameRate;
use crate::track::{Track, TrackItem, TrackKind};

/// The position of a track within a [`Timeline`], returned by
/// [`Timeline::add_track`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackIndex(usize);

impl TrackIndex {
    /// The zero-based position.
    pub fn get(self) -> usize {
        self.0
    }
}

/// A multi-track timeline: media assets plus the tracks that place clips of them.
///
/// The timeline owns cross-reference validation — every clip must point at a
/// registered asset and stay within its bounds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timeline {
    name: String,
    frame_rate: FrameRate,
    assets: Vec<MediaAsset>,
    tracks: Vec<Track>,
}

impl Timeline {
    /// An empty timeline at the given global frame rate.
    pub fn new(name: impl Into<String>, frame_rate: FrameRate) -> Self {
        Self {
            name: name.into(),
            frame_rate,
            assets: Vec::new(),
            tracks: Vec::new(),
        }
    }

    /// The timeline's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The timeline's global frame rate.
    pub fn frame_rate(&self) -> FrameRate {
        self.frame_rate
    }

    /// The registered assets.
    pub fn assets(&self) -> &[MediaAsset] {
        &self.assets
    }

    /// The tracks, in order.
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Register a media asset. Errors on a duplicate id.
    pub fn add_asset(&mut self, asset: MediaAsset) -> Result<(), TimelineError> {
        if self.asset(asset.source()).is_some() {
            return Err(TimelineError::DuplicateAsset(asset.source().clone()));
        }
        self.assets.push(asset);
        Ok(())
    }

    /// Look up a registered asset by its source.
    pub fn asset(&self, source: &MediaSource) -> Option<&MediaAsset> {
        self.assets.iter().find(|asset| asset.source() == source)
    }

    /// Append a track and return its index.
    pub fn add_track(&mut self, track: Track) -> TrackIndex {
        let index = TrackIndex(self.tracks.len());
        self.tracks.push(track);
        index
    }

    /// The track at `index`, if it exists.
    pub fn track(&self, index: TrackIndex) -> Option<&Track> {
        self.tracks.get(index.0)
    }

    /// Validate the whole timeline: each track is structurally sound, every clip
    /// references a registered asset, no clip's source range exceeds its asset's
    /// duration, and each clip's track kind matches a stream its asset carries.
    pub fn validate(&self) -> Result<(), TimelineError> {
        let assets_by_source: HashMap<&MediaSource, &MediaAsset> = self
            .assets
            .iter()
            .map(|asset| (asset.source(), asset))
            .collect();
        for track in &self.tracks {
            track.validate()?;
            for item in track.items() {
                if let TrackItem::Clip(clip) = item {
                    let asset = assets_by_source
                        .get(clip.asset())
                        .ok_or_else(|| TimelineError::UnknownAsset(clip.asset().clone()))?;
                    let range = clip.range();
                    if range.duration().is_zero() {
                        return Err(TimelineError::EmptyClip);
                    }
                    if range.start().is_negative() || range.end() > asset.duration() {
                        return Err(TimelineError::ClipOutOfAssetBounds);
                    }
                    let has_required_stream = match track.kind() {
                        TrackKind::Video => asset.video().is_some(),
                        TrackKind::Audio => asset.audio().is_some(),
                    };
                    if !has_required_stream {
                        return Err(TimelineError::TrackAssetStreamMismatch);
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{AudioProperties, ChannelLayout, VideoProperties};
    use crate::time::{SampleRate, Seconds, TimeRange};
    use crate::track::{Clip, Track, TrackKind};

    fn audio_asset(id: &str, seconds: i64) -> MediaAsset {
        MediaAsset::new(
            MediaSource::file(id),
            Seconds::from_secs(seconds),
            None,
            Some(AudioProperties {
                sample_rate: SampleRate::new(48_000).unwrap(),
                channels: ChannelLayout::Stereo,
            }),
        )
        .unwrap()
    }

    fn video_asset(id: &str, seconds: i64) -> MediaAsset {
        MediaAsset::new(
            MediaSource::file(id),
            Seconds::from_secs(seconds),
            Some(VideoProperties {
                frame_rate: fps(),
                width: 1920,
                height: 1080,
            }),
            None,
        )
        .unwrap()
    }

    fn fps() -> FrameRate {
        FrameRate::whole(30).unwrap()
    }

    #[test]
    fn duplicate_asset_is_rejected() {
        let mut timeline = Timeline::new("t", fps());
        timeline.add_asset(audio_asset("a", 10)).unwrap();
        assert_eq!(
            timeline.add_asset(audio_asset("a", 5)),
            Err(TimelineError::DuplicateAsset(MediaSource::file("a")))
        );
    }

    #[test]
    fn clip_referencing_unknown_asset_fails_validation() {
        let mut timeline = Timeline::new("t", fps());
        let mut track = Track::new(TrackKind::Audio);
        let range = TimeRange::from_origin(Seconds::from_secs(2)).unwrap();
        track.push_clip(Clip::new(MediaSource::file("missing"), range));
        timeline.add_track(track);
        assert_eq!(
            timeline.validate(),
            Err(TimelineError::UnknownAsset(MediaSource::file("missing")))
        );
    }

    #[test]
    fn clip_exceeding_asset_duration_fails_validation() {
        let mut timeline = Timeline::new("t", fps());
        timeline.add_asset(audio_asset("a", 3)).unwrap();
        let mut track = Track::new(TrackKind::Audio);
        let range = TimeRange::new(Seconds::from_secs(2), Seconds::from_secs(2)).unwrap();
        track.push_clip(Clip::new(MediaSource::file("a"), range));
        timeline.add_track(track);
        assert_eq!(
            timeline.validate(),
            Err(TimelineError::ClipOutOfAssetBounds)
        );
    }

    #[test]
    fn clip_starting_before_the_asset_fails_validation() {
        let mut timeline = Timeline::new("t", fps());
        timeline.add_asset(audio_asset("a", 10)).unwrap();
        let mut track = Track::new(TrackKind::Audio);
        // start = -1s, duration = 2s: a valid TimeRange but out of asset bounds.
        let range = TimeRange::new(Seconds::new(-1, 1).unwrap(), Seconds::from_secs(2)).unwrap();
        track.push_clip(Clip::new(MediaSource::file("a"), range));
        timeline.add_track(track);
        assert_eq!(
            timeline.validate(),
            Err(TimelineError::ClipOutOfAssetBounds)
        );
    }

    #[test]
    fn zero_duration_clip_fails_validation() {
        let mut timeline = Timeline::new("t", fps());
        timeline.add_asset(audio_asset("a", 10)).unwrap();
        let mut track = Track::new(TrackKind::Audio);
        let range = TimeRange::new(Seconds::from_secs(1), Seconds::ZERO).unwrap();
        track.push_clip(Clip::new(MediaSource::file("a"), range));
        timeline.add_track(track);
        assert_eq!(timeline.validate(), Err(TimelineError::EmptyClip));
    }

    #[test]
    fn well_formed_timeline_validates() {
        let mut timeline = Timeline::new("t", fps());
        timeline.add_asset(audio_asset("a", 10)).unwrap();
        let mut track = Track::new(TrackKind::Audio);
        let range = TimeRange::new(Seconds::from_secs(1), Seconds::from_secs(4)).unwrap();
        track.push_clip(Clip::new(MediaSource::file("a"), range));
        timeline.add_track(track);
        assert_eq!(timeline.validate(), Ok(()));
    }

    #[test]
    fn video_clip_from_audio_only_asset_fails_validation() {
        let mut timeline = Timeline::new("t", fps());
        timeline.add_asset(audio_asset("a", 10)).unwrap();
        let mut track = Track::new(TrackKind::Video);
        let range = TimeRange::new(Seconds::ZERO, Seconds::from_secs(2)).unwrap();
        track.push_clip(Clip::new(MediaSource::file("a"), range));
        timeline.add_track(track);
        assert_eq!(
            timeline.validate(),
            Err(TimelineError::TrackAssetStreamMismatch)
        );
    }

    #[test]
    fn video_clip_from_video_asset_validates() {
        let mut timeline = Timeline::new("t", fps());
        timeline.add_asset(video_asset("v", 10)).unwrap();
        let mut track = Track::new(TrackKind::Video);
        let range = TimeRange::new(Seconds::from_secs(1), Seconds::from_secs(4)).unwrap();
        track.push_clip(Clip::new(MediaSource::file("v"), range));
        timeline.add_track(track);
        assert_eq!(timeline.validate(), Ok(()));
    }

    #[test]
    fn track_accessor_resolves_the_returned_index() {
        let mut timeline = Timeline::new("t", fps());
        let index = timeline.add_track(Track::new(TrackKind::Audio));
        assert_eq!(
            timeline.track(index).map(Track::kind),
            Some(TrackKind::Audio)
        );
    }
}
