//! FFmpeg-backed media probing and decoding for Hollywood, behind backend-
//! swappable traits.
//!
//! Hollywood links FFmpeg directly via `ffmpeg-next`. [`FfmpegProbe`] reads a
//! source's duration and stream properties into [`ProbedMedia`], which converts
//! to a timeline [`hollywood_timeline::MediaAsset`]; [`FfmpegDecoder`] decodes a
//! source's audio into a mono [`MonoAudio`] buffer for analysis. The
//! [`MediaProbe`] and [`DecodeAudio`] traits keep callers backend-agnostic, so a
//! pure-Rust backend (Symphonia) can replace either without touching the pipeline.
//!
//! Callers must invoke [`ffmpeg_next::init`] once at process startup before any
//! probe or decode — the root binary's `media::init` wrapper handles this for
//! the app.

pub mod decode;
pub mod probe;

mod error;

pub use decode::{DecodeAudio, FfmpegDecoder, MonoAudio};
pub use error::MediaError;
pub use probe::{FfmpegProbe, MediaProbe, ProbedMedia};
