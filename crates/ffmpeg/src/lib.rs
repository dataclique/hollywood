//! FFmpeg-backed media probing for Hollywood, behind a backend-swappable trait.
//!
//! Hollywood links FFmpeg directly via `ffmpeg-next`. [`FfmpegProbe`] reads a
//! source's duration and stream properties into [`ProbedMedia`], which converts
//! to a timeline [`hollywood_timeline::MediaAsset`]. The [`MediaProbe`] trait
//! keeps callers backend-agnostic, so a pure-Rust backend (Symphonia) can be
//! swapped in without touching the pipeline.

pub mod probe;

mod error;

pub use error::MediaError;
pub use probe::{FfmpegProbe, MediaProbe, ProbedMedia};
