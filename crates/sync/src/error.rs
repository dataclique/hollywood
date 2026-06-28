//! Errors produced by the sync stage.

use thiserror::Error;

/// Failure aligning two signals.
#[derive(Debug, Error)]
pub enum SyncError {
    /// One of the signals to align was empty; there is nothing to correlate.
    #[error("cannot align an empty signal")]
    EmptySignal,
    /// The signals are too long for their combined length to be represented.
    #[error("signal is too long to cross-correlate")]
    SignalTooLong,
    /// The cross-correlation has no positive peak, so the signals are silent or
    /// uncorrelated and no meaningful offset exists.
    #[error("signals have no correlation peak")]
    NoPeak,
    /// Neither signal is long enough to hold a single drift-map window, so no
    /// alignment can be measured.
    #[error("signal is shorter than the drift-map window")]
    SignalShorterThanWindow,
    /// The underlying FFT failed.
    #[error("FFT failed: {0}")]
    Fft(#[from] realfft::FftError),
}
