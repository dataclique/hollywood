//! Errors produced by the NLE exporters.

use thiserror::Error;

/// Failure serializing a timeline to an NLE interchange format.
#[derive(Debug, Error)]
pub enum NleError {
    /// The XML writer failed.
    #[error("xml serialization failed: {0}")]
    Xml(String),

    /// The serialized bytes were not valid UTF-8.
    #[error("serialized output was not valid utf-8: {0}")]
    Encoding(String),

    /// The timeline uses a fractional (NTSC) frame rate, which the xmeml
    /// exporter does not handle yet.
    #[error("non-integer (NTSC) frame rates are not yet supported by the xmeml exporter")]
    UnsupportedFrameRate,

    /// The timeline contains a transition; only hard cuts are supported so far.
    #[error("transitions are not yet supported by the xmeml exporter")]
    UnsupportedTransition,
}
