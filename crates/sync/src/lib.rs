//! Multi-source audio alignment for Hollywood.
//!
//! When a scene is captured by more than one device (a camera's on-board mic and
//! a separate recorder), the recordings start at different instants. [`align`]
//! cross-correlates two mono signals and reports the [`SyncOffset`] between them
//! — by how many samples one lags the other — so the assembler can place clips
//! from each source on a shared timebase.
//!
//! Correlation is done in the frequency domain (`realfft` over `rustfft`), which
//! is `O(n log n)` rather than the `O(n²)` of a direct sum. Like the detector,
//! it works on raw `&[f32]` samples plus a [`hollywood_timeline::SampleRate`],
//! so it is independent of how the audio was decoded.

mod alignment;
mod error;

pub use alignment::{SyncOffset, align};
pub use error::SyncError;
