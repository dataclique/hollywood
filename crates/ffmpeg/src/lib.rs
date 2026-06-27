//! FFmpeg-backed media probing for Hollywood, behind a backend-swappable trait.
//!
//! Hollywood links FFmpeg directly via `ffmpeg-next`. [`FfmpegProbe`] reads a
//! source's duration and stream properties into [`ProbedMedia`], which converts
//! to a timeline [`hollywood_timeline::MediaAsset`]. The [`MediaProbe`] trait
//! keeps callers backend-agnostic, so a pure-Rust backend (Symphonia) can be
//! swapped in without touching the pipeline.
//!
//! Callers must invoke [`ffmpeg_next::init`] once at process startup before any
//! probe — the root binary's `media::init` wrapper handles this for the app.

pub mod decode;
pub mod probe;

mod error;

pub use decode::{DecodeAudio, FfmpegDecoder, MonoAudio};
pub use error::MediaError;
pub use probe::{FfmpegProbe, MediaProbe, ProbedMedia};
