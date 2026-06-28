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
    /// The cross-correlation produced no samples to take a peak from.
    #[error("cross-correlation produced no peak")]
    NoPeak,
    /// The underlying FFT failed.
    #[error("FFT failed: {0}")]
    Fft(#[from] realfft::FftError),
}
