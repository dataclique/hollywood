//! Hand-written NLE interchange exporters for the Hollywood timeline IR.
//!
//! Each exporter projects a [`hollywood_timeline::Timeline`] onto a specific
//! NLE interchange format: FCP7 [`xmeml`] (the format that opens natively in
//! both Premiere and Resolve), [`fcpxml`] (the format Final Cut Pro and DaVinci
//! Resolve prefer), and [`otio`] (OpenTimelineIO JSON, an optional interchange
//! path).

pub mod fcpxml;
pub mod otio;
pub mod xmeml;

mod error;

pub use error::NleError;
pub use fcpxml::to_fcpxml;
pub use otio::to_otio;
pub use xmeml::to_xmeml;
