//! Tracks: ordered lanes of clips, gaps, and transitions.
//!
//! A track is a sequence; a clip's timeline position is implied by the items
//! before it (clips and gaps occupy time; transitions overlap their
//! neighbours). This mirrors the FCP7/xmeml track model.

use crate::asset::MediaSource;
use crate::error::TimelineError;
use crate::time::{Seconds, TimeRange};

/// Whether a track carries video or audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    /// A video track.
    Video,
    /// An audio track.
    Audio,
}

/// A placed reference to a span of a media asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    asset: MediaSource,
    range: TimeRange,
    name: Option<String>,
}

impl Clip {
    /// A clip of `range` from `asset`.
    pub fn new(asset: MediaSource, range: TimeRange) -> Self {
        Self {
            asset,
            range,
            name: None,
        }
    }

    /// A clip with an explicit name (used by NLE exporters for relinking).
    pub fn with_name(asset: MediaSource, range: TimeRange, name: impl Into<String>) -> Self {
        Self {
            asset,
            range,
            name: Some(name.into()),
        }
    }

    /// The asset this clip references.
    pub fn asset(&self) -> &MediaSource {
        &self.asset
    }

    /// The in/out range into the asset.
    pub fn range(&self) -> TimeRange {
        self.range
    }

    /// The clip's name, if set.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// How long the clip occupies the track.
    pub fn duration(&self) -> Seconds {
        self.range.duration()
    }
}

/// Empty space on a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gap {
    duration: Seconds,
}

impl Gap {
    /// A gap of the given strictly-positive duration — a gap must occupy time.
    pub fn new(duration: Seconds) -> Result<Self, TimelineError> {
        if duration.is_negative() || duration.is_zero() {
            return Err(TimelineError::NonPositiveDuration);
        }
        Ok(Self { duration })
    }

    /// The gap's length.
    pub fn duration(self) -> Seconds {
        self.duration
    }
}

/// An audio cross-fade between the two clips it sits between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transition {
    duration: Seconds,
}

impl Transition {
    /// A cross-fade of the given strictly-positive duration.
    pub fn cross_fade(duration: Seconds) -> Result<Self, TimelineError> {
        if duration.is_negative() || duration.is_zero() {
            return Err(TimelineError::NonPositiveDuration);
        }
        Ok(Self { duration })
    }

    /// The transition's length.
    pub fn duration(self) -> Seconds {
        self.duration
    }
}

/// One element in a track's sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrackItem {
    /// A placed clip.
    Clip(Clip),
    /// Empty space.
    Gap(Gap),
    /// A transition between the surrounding clips.
    Transition(Transition),
}

/// An ordered lane of clips, gaps, and transitions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    kind: TrackKind,
    items: Vec<TrackItem>,
}

impl Track {
    /// An empty track of the given kind.
    pub fn new(kind: TrackKind) -> Self {
        Self {
            kind,
            items: Vec::new(),
        }
    }

    /// The track's kind.
    pub fn kind(&self) -> TrackKind {
        self.kind
    }

    /// The track's items in order.
    pub fn items(&self) -> &[TrackItem] {
        &self.items
    }

    /// Append a clip.
    pub fn push_clip(&mut self, clip: Clip) {
        self.items.push(TrackItem::Clip(clip));
    }

    /// Append a gap.
    pub fn push_gap(&mut self, gap: Gap) {
        self.items.push(TrackItem::Gap(gap));
    }

    /// Append a transition. Errors unless the preceding item is a clip; the
    /// following clip is checked by [`Track::validate`].
    pub fn push_transition(&mut self, transition: Transition) -> Result<(), TimelineError> {
        if !matches!(self.items.last(), Some(TrackItem::Clip(_))) {
            return Err(TimelineError::MisplacedTransition);
        }
        self.items.push(TrackItem::Transition(transition));
        Ok(())
    }

    /// Time occupied on the track. Clips and gaps add length; transitions
    /// overlap their neighbours and contribute none.
    pub fn occupied(&self) -> Seconds {
        self.items
            .iter()
            .fold(Seconds::ZERO, |acc, item| match item {
                TrackItem::Clip(clip) => acc + clip.duration(),
                TrackItem::Gap(gap) => acc + gap.duration(),
                TrackItem::Transition(_) => acc,
            })
    }

    /// Validate structure: every transition sits between two clips.
    pub fn validate(&self) -> Result<(), TimelineError> {
        for (index, item) in self.items.iter().enumerate() {
            if matches!(item, TrackItem::Transition(_)) {
                let prev_is_clip = index
                    .checked_sub(1)
                    .and_then(|p| self.items.get(p))
                    .is_some_and(|it| matches!(it, TrackItem::Clip(_)));
                let next_is_clip = self
                    .items
                    .get(index.saturating_add(1))
                    .is_some_and(|it| matches!(it, TrackItem::Clip(_)));
                if !prev_is_clip || !next_is_clip {
                    return Err(TimelineError::MisplacedTransition);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(seconds: i64) -> Clip {
        let range = TimeRange::from_origin(Seconds::from_secs(seconds)).unwrap();
        Clip::new(MediaSource::file("a.mov"), range)
    }

    #[test]
    fn occupied_sums_clips_and_gaps_but_not_transitions() {
        let mut track = Track::new(TrackKind::Audio);
        track.push_clip(clip(3));
        track
            .push_transition(Transition::cross_fade(Seconds::from_secs(1)).unwrap())
            .unwrap();
        track.push_clip(clip(2));
        track.push_gap(Gap::new(Seconds::from_secs(4)).unwrap());
        assert_eq!(track.occupied(), Seconds::from_secs(9));
    }

    #[test]
    fn transition_must_follow_a_clip() {
        let mut track = Track::new(TrackKind::Audio);
        let result = track.push_transition(Transition::cross_fade(Seconds::from_secs(1)).unwrap());
        assert_eq!(result, Err(TimelineError::MisplacedTransition));
    }

    #[test]
    fn transition_must_precede_a_clip() {
        let mut track = Track::new(TrackKind::Audio);
        track.push_clip(clip(2));
        track
            .push_transition(Transition::cross_fade(Seconds::from_secs(1)).unwrap())
            .unwrap();
        // No clip after the transition: validation must fail.
        assert_eq!(track.validate(), Err(TimelineError::MisplacedTransition));
    }

    #[test]
    fn well_formed_track_validates() {
        let mut track = Track::new(TrackKind::Audio);
        track.push_clip(clip(2));
        track
            .push_transition(Transition::cross_fade(Seconds::from_secs(1)).unwrap())
            .unwrap();
        track.push_clip(clip(2));
        assert_eq!(track.validate(), Ok(()));
    }

    #[test]
    fn zero_cross_fade_is_rejected() {
        assert_eq!(
            Transition::cross_fade(Seconds::ZERO),
            Err(TimelineError::NonPositiveDuration)
        );
    }

    #[test]
    fn zero_and_negative_gaps_are_rejected() {
        assert_eq!(
            Gap::new(Seconds::ZERO),
            Err(TimelineError::NonPositiveDuration)
        );
        assert_eq!(
            Gap::new(Seconds::new(-1, 1).unwrap()),
            Err(TimelineError::NonPositiveDuration)
        );
    }
}
