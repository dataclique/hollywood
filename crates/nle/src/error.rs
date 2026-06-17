//! Errors produced by the NLE exporters.

use hollywood_timeline::TimelineError;
use thiserror::Error;

/// Failure serializing a timeline to an NLE interchange format.
#[derive(Debug, Error)]
pub enum NleError {
    /// The timeline failed its own validation, so it must not be exported.
    #[error("timeline is invalid and cannot be exported: {0}")]
    InvalidTimeline(#[from] TimelineError),

    /// A clip or gap duration is not an exact whole number of frames at the
    /// sequence rate, so it cannot be placed without silently snapping.
    #[error("duration is not frame-aligned at the sequence rate")]
    UnalignedDuration,

    /// The XML writer failed.
    #[error("xml serialization failed: {0}")]
    Xml(#[from] std::io::Error),

    /// The serialized bytes were not valid UTF-8.
    #[error("serialized output was not valid utf-8: {0}")]
    Encoding(#[from] std::string::FromUtf8Error),

    /// The timeline uses a fractional (NTSC) frame rate, which the xmeml
    /// exporter does not handle yet.
    #[error("non-integer (NTSC) frame rates are not yet supported by the xmeml exporter")]
    UnsupportedFrameRate,

    /// The timeline contains a transition; only hard cuts are supported so far.
    #[error("transitions are not yet supported by the xmeml exporter")]
    UnsupportedTransition,
}
