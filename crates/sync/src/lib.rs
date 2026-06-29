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
//!
//! Callers pick a [`CorrelationMethod`]: [`CorrelationMethod::CrossCorrelation`]
//! for plain correlation, or [`CorrelationMethod::Phat`] (GCC-PHAT) to whiten the
//! spectrum for a sharp, amplitude-invariant peak on colored or low-SNR material.
//!
//! One offset assumes the two clocks tick at the same rate. Over a long take they
//! drift, so [`drift_map`] aligns successive windows into a [`DriftMap`] — the
//! offset sampled over time — letting the assembler correct drift rather than
//! assume a fixed lag.

mod alignment;
mod drift;
mod error;

pub use alignment::{CorrelationMethod, SyncOffset, align};
pub use drift::{DriftMap, DriftPoint, DriftWindow, drift_map};
pub use error::SyncError;
