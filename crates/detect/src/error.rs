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
    /// The silence threshold does not convert to a finite, positive amplitude
    /// (e.g. a non-finite dBFS, or one so extreme it underflows to zero or
    /// overflows to infinity), which would invert or disable gating.
    #[error("silence threshold must convert to a finite, positive amplitude")]
    InvalidThreshold,
    /// The sample buffer contains a non-finite value (NaN or infinity), which
    /// would silently misclassify windows rather than gate them.
    #[error("audio samples contain a non-finite value")]
    NonFiniteSample,
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
