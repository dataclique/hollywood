//! Errors produced by the sync stage.

use thiserror::Error;

/// Failure aligning two signals.
#[derive(Debug, Error)]
pub enum SyncError {
    /// One of the signals to align was empty; there is nothing to correlate.
    #[error("cannot align an empty signal")]
    EmptySignal,
    /// A length or offset is too large to represent in the sample-count integer:
    /// the signals' combined length, or a composed `base + residual` drift
    /// offset, overflows `i64`.
    #[error("signal or offset is too long to represent")]
    SignalTooLong,
    /// The cross-correlation has no positive peak, so the signals are silent or
    /// uncorrelated and no meaningful offset exists.
    #[error("signals have no correlation peak")]
    NoPeak,
    /// `reference` is not long enough to hold a single drift-map window, so no
    /// alignment can be measured. (A `target` too short to hold a window instead
    /// surfaces as [`NoWindowInBounds`](Self::NoWindowInBounds).)
    #[error("reference is shorter than the drift-map window")]
    SignalShorterThanWindow,
    /// Every drift-map window's base-shifted span fell outside `target`, so no
    /// window could be correlated — the `base` offset is inconsistent with the
    /// signals' lengths, or `target` is shorter than a single window. Distinct
    /// from [`NoPeak`](Self::NoPeak), where windows *were* correlated but none
    /// produced a peak.
    #[error("no drift-map window's base-shifted span falls within the target")]
    NoWindowInBounds,
    /// The underlying FFT failed.
    #[error("FFT failed: {0}")]
    Fft(#[from] realfft::FftError),
}
