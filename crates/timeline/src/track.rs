//! Tracks: ordered lanes of clips, gaps, and transitions.
//!
//! A track is a sequence; a clip's timeline position is the running sum of the
//! clip and gap durations before it. A transition is a render-time overlay on
//! the boundary between its two clips — it does not advance the track position.
//! This mirrors the FCP7/xmeml track model.

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

impl TrackItem {
    pub(crate) fn as_clip(&self) -> Option<&Clip> {
        match self {
            Self::Clip(clip) => Some(clip),
            Self::Gap(_) | Self::Transition(_) => None,
        }
    }
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

    /// Append a gap. Errors if the preceding item is a transition, which would
    /// strand that transition without a following clip.
    pub fn push_gap(&mut self, gap: Gap) -> Result<(), TimelineError> {
        if matches!(self.items.last(), Some(TrackItem::Transition(_))) {
            return Err(TimelineError::MisplacedTransition);
        }
        self.items.push(TrackItem::Gap(gap));
        Ok(())
    }

    /// Append a transition. Errors unless this is an audio track and the
    /// preceding item is a clip; the following clip is checked by
    /// [`Track::validate`].
    pub fn push_transition(&mut self, transition: Transition) -> Result<(), TimelineError> {
        if !matches!(self.kind, TrackKind::Audio) {
            return Err(TimelineError::TransitionOnVideoTrack);
        }
        if !matches!(self.items.last(), Some(TrackItem::Clip(_))) {
            return Err(TimelineError::MisplacedTransition);
        }
        self.items.push(TrackItem::Transition(transition));
        Ok(())
    }

    /// The total time the track occupies: the running sum of clip and gap
    /// durations. A transition is a render-time overlay on a clip boundary and
    /// does not advance the track position, so it adds nothing to this total.
    /// Errors if the sum is not representable in exact `i64` rational seconds.
    pub fn occupied(&self) -> Result<Seconds, TimelineError> {
        self.items.iter().try_fold(Seconds::ZERO, |acc, item| {
            let span = match item {
                TrackItem::Clip(clip) => clip.duration(),
                TrackItem::Gap(gap) => gap.duration(),
                TrackItem::Transition(_) => return Ok(acc),
            };
            acc.checked_add(span).ok_or(TimelineError::OccupiedOverflow)
        })
    }

    /// Validate structure: the occupied duration is representable, every
    /// transition sits between two clips, and no cross-fade is longer than
    /// either clip it joins (a fade overlaps its neighbours, so it cannot
    /// consume more of one than exists).
    pub fn validate(&self) -> Result<(), TimelineError> {
        self.occupied()?;
        self.items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| match item {
                TrackItem::Transition(transition) => Some((index, transition)),
                TrackItem::Clip(_) | TrackItem::Gap(_) => None,
            })
            .try_for_each(|(index, transition)| self.validate_transition(index, transition))
    }

    /// Validate the transition at `index`: it must sit between two clips, and as
    /// an overlap it cannot be longer than either clip it joins.
    fn validate_transition(
        &self,
        index: usize,
        transition: &Transition,
    ) -> Result<(), TimelineError> {
        let prev_clip = index
            .checked_sub(1)
            .and_then(|prev| self.items.get(prev))
            .and_then(TrackItem::as_clip);
        let next_clip = index
            .checked_add(1)
            .and_then(|next| self.items.get(next))
            .and_then(TrackItem::as_clip);
        let (Some(prev_clip), Some(next_clip)) = (prev_clip, next_clip) else {
            return Err(TimelineError::MisplacedTransition);
        };
        let fade = transition.duration();
        if fade > prev_clip.duration() || fade > next_clip.duration() {
            return Err(TimelineError::CrossFadeTooLong);
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
        track
            .push_gap(Gap::new(Seconds::from_secs(4)).unwrap())
            .unwrap();
        assert_eq!(track.occupied(), Ok(Seconds::from_secs(9)));
    }

    #[test]
    fn transition_must_follow_a_clip() {
        let mut track = Track::new(TrackKind::Audio);
        let result = track.push_transition(Transition::cross_fade(Seconds::from_secs(1)).unwrap());
        assert_eq!(result, Err(TimelineError::MisplacedTransition));
    }

    #[test]
    fn transition_is_rejected_on_a_video_track() {
        let mut track = Track::new(TrackKind::Video);
        track.push_clip(clip(2));
        let result = track.push_transition(Transition::cross_fade(Seconds::from_secs(1)).unwrap());
        assert_eq!(result, Err(TimelineError::TransitionOnVideoTrack));
    }

    #[test]
    fn gap_is_rejected_immediately_after_a_transition() {
        let mut track = Track::new(TrackKind::Audio);
        track.push_clip(clip(2));
        track
            .push_transition(Transition::cross_fade(Seconds::from_secs(1)).unwrap())
            .unwrap();
        assert_eq!(
            track.push_gap(Gap::new(Seconds::from_secs(1)).unwrap()),
            Err(TimelineError::MisplacedTransition)
        );
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
    fn new_clip_has_no_name() {
        assert_eq!(clip(2).name(), None);
    }

    #[test]
    fn cross_fade_longer_than_a_clip_is_rejected() {
        let mut track = Track::new(TrackKind::Audio);
        track.push_clip(clip(2));
        track
            .push_transition(Transition::cross_fade(Seconds::from_secs(3)).unwrap())
            .unwrap();
        track.push_clip(clip(5));
        // The 3s fade exceeds the 2s first clip it overlaps.
        assert_eq!(track.validate(), Err(TimelineError::CrossFadeTooLong));
    }

    #[test]
    fn occupied_overflows_on_unrepresentable_total() {
        let huge = TimeRange::from_origin(Seconds::new(i64::MAX, 1).unwrap()).unwrap();
        let mut track = Track::new(TrackKind::Video);
        track.push_clip(Clip::new(MediaSource::file("a.mov"), huge));
        track.push_clip(Clip::new(MediaSource::file("b.mov"), huge));
        assert_eq!(track.occupied(), Err(TimelineError::OccupiedOverflow));
        // validate() is the documented gate, so it surfaces the same overflow.
        assert_eq!(track.validate(), Err(TimelineError::OccupiedOverflow));
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
