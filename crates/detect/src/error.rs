//! Errors produced by the detection stage.

use hollywood_timeline::TimelineError;
use thiserror::Error;

/// Failure configuring or running silence detection.
#[derive(Debug, Error)]
pub enum DetectError {
    /// The analysis window is zero or negative; it must occupy time.
    #[error("analysis window must be a positive duration")]
    NonPositiveWindow,
    /// The keep-region padding is negative; it must be zero or positive.
    #[error("padding must not be negative")]
    NegativePadding,
    /// The silence threshold is not a finite level.
    #[error("silence threshold must be a finite dBFS value")]
    NonFiniteThreshold,
    /// The analysis window spans fewer than one sample at the signal's rate (or
    /// is unrepresentably large), so it cannot be analyzed.
    #[error("analysis window does not span a usable number of samples at this rate")]
    InvalidWindow,
    /// A computed region duration overflowed exact rational seconds.
    #[error("region duration overflowed representable time")]
    DurationOverflow,
    /// The timeline IR rejected a computed region.
    #[error("timeline IR rejected a detected region: {0}")]
    Timeline(#[from] TimelineError),
}
