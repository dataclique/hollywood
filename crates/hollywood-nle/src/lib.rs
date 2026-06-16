//! Hand-written NLE interchange exporters for the Hollywood timeline IR.
//!
//! Each exporter projects a [`hollywood_timeline::Timeline`] onto a specific
//! NLE interchange format. FCP7 [`xmeml`] (the format that opens natively in
//! both Premiere and Resolve) is the primary target; the FCPXML exporter
//! (Final Cut / Resolve) lands in a follow-up.

pub mod xmeml;

mod error;

pub use error::NleError;
pub use xmeml::to_xmeml;
