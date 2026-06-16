//! Errors produced when constructing or validating timeline IR values.

use thiserror::Error;

use crate::asset::MediaSource;

/// Failure building or validating a timeline IR value.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum TimelineError {
    /// A rational time was constructed with a zero denominator.
    #[error("time denominator must be non-zero")]
    ZeroDenominator,

    /// A rational time was constructed with a negative denominator. The sign
    /// belongs on the numerator, so this is rejected rather than silently
    /// normalized.
    #[error("time denominator must be positive")]
    NegativeDenominator,

    /// A frame rate numerator or denominator was zero.
    #[error("frame rate must be positive")]
    NonPositiveFrameRate,

    /// A sample rate was zero.
    #[error("sample rate must be non-zero")]
    ZeroSampleRate,

    /// An explicit channel count was zero.
    #[error("channel count must be non-zero")]
    ZeroChannelCount,

    /// A duration was negative where a non-negative value is required.
    #[error("duration must be non-negative")]
    NegativeDuration,

    /// A duration was zero or negative where a strictly positive value is
    /// required (gaps and transitions must occupy time).
    #[error("duration must be positive")]
    NonPositiveDuration,

    /// A media asset was constructed with neither a video nor an audio stream —
    /// it carries no media to place clips from or export.
    #[error("a media asset must carry at least one stream")]
    AssetWithoutStreams,

    /// Two assets were registered under the same source.
    #[error("duplicate asset: {0}")]
    DuplicateAsset(MediaSource),

    /// A clip referenced an asset not registered in the timeline.
    #[error("clip references unknown asset: {0}")]
    UnknownAsset(MediaSource),

    /// A clip's source range has zero duration.
    #[error("a clip's source range must have positive duration")]
    EmptyClip,

    /// A clip's source range lies outside its asset — it starts before zero or
    /// ends after the asset's duration.
    #[error("a clip's source range must lie within its asset")]
    ClipOutOfAssetBounds,

    /// A time range's exclusive end (`start + duration`) is not representable in
    /// exact `i64` rational seconds.
    #[error("a time range's end is not representable")]
    TimeRangeOverflow,

    /// The summed duration of a track's items is not representable in exact
    /// `i64` rational seconds.
    #[error("a track's occupied duration is not representable")]
    OccupiedOverflow,

    /// A transition was placed somewhere other than between two clips.
    #[error("a transition must sit between two clips")]
    MisplacedTransition,

    /// An audio cross-fade was placed on a track that is not an audio track.
    #[error("a cross-fade can only sit on an audio track")]
    TransitionOnVideoTrack,

    /// A cross-fade is longer than one of the clips it sits between — it would
    /// consume more of a neighbour than exists.
    #[error("a cross-fade cannot be longer than the clips it joins")]
    CrossFadeTooLong,

    /// A clip's track kind does not match the streams its asset carries — a
    /// video clip from an asset with no video stream, or an audio clip from an
    /// asset with no audio stream.
    #[error("a clip's track kind does not match its asset's streams")]
    TrackAssetStreamMismatch,
}
