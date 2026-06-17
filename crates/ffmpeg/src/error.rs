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

    /// A stream's frame rate was zero or otherwise invalid.
    #[error("media reported an invalid frame rate")]
    InvalidFrameRate,

    /// A stream's sample rate was zero.
    #[error("media reported an invalid sample rate")]
    InvalidSampleRate,

    /// A stream reported a zero/unreadable channel count — channel metadata was
    /// unavailable, not genuinely mono.
    #[error("media reported an invalid channel layout")]
    InvalidChannelLayout,

    /// The probed properties could not form a valid timeline asset.
    #[error("probed media is not a valid asset: {0}")]
    Asset(#[from] TimelineError),
}
