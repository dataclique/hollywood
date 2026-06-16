//! Errors produced when constructing or validating timeline IR values.

use thiserror::Error;

use crate::asset::MediaSource;

/// Failure building or validating a timeline IR value.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum TimelineError {
    /// A rational time was constructed with a zero denominator.
    #[error("time denominator must be non-zero")]
    ZeroDenominator,

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

    /// A transition was placed somewhere other than between two clips.
    #[error("a transition must sit between two clips")]
    MisplacedTransition,
}
