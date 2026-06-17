//! Errors produced by the media backend.

use hollywood_timeline::TimelineError;
use thiserror::Error;

/// Failure probing or decoding a media source.
#[derive(Debug, Error)]
pub enum MediaError {
    /// FFmpeg failed to open, demux, or decode the source.
    #[error("ffmpeg failed: {0}")]
    Ffmpeg(#[from] ffmpeg_next::Error),
    /// The source has neither a video nor an audio stream.
    #[error("media has no video or audio streams")]
    NoStreams,
    /// Neither the container nor any stream reported a usable duration.
    #[error("media reported no usable duration")]
    UnknownDuration,
    /// Stream metadata does not fit a typed domain bound (e.g. frame-rate
    /// numerator overflowed `u32`).
    #[error("stream metadata does not fit typed domain bounds: {0}")]
    MetadataBounds(#[from] std::num::TryFromIntError),
    /// The timeline IR rejected probed or converted values.
    #[error("timeline IR rejected probed values: {0}")]
    Timeline(#[from] TimelineError),
}
