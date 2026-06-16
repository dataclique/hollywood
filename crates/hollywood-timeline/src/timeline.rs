//! The timeline: a registry of media assets and the tracks that reference them.

use crate::asset::{MediaAsset, MediaSource};
use crate::error::TimelineError;
use crate::time::FrameRate;
use crate::track::{Track, TrackItem};

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
    pub fn add_track(&mut self, track: Track) -> usize {
        let index = self.tracks.len();
        self.tracks.push(track);
        index
    }

    /// Validate the whole timeline: each track is structurally sound, every
    /// clip references a registered asset, and no clip's source range exceeds
    /// its asset's duration.
    pub fn validate(&self) -> Result<(), TimelineError> {
        for track in &self.tracks {
            track.validate()?;
            for item in track.items() {
                if let TrackItem::Clip(clip) = item {
                    let asset = self
                        .asset(clip.asset())
                        .ok_or_else(|| TimelineError::UnknownAsset(clip.asset().clone()))?;
                    let range = clip.range();
                    if range.duration().is_zero() {
                        return Err(TimelineError::EmptyClip);
                    }
                    if range.start().is_negative() || range.end() > asset.duration() {
                        return Err(TimelineError::ClipOutOfAssetBounds);
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
    use crate::asset::{AudioProperties, ChannelLayout};
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
}
