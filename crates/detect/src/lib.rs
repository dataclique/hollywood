//! Silence detection for Hollywood: decide which spans of a recording to keep.
//!
//! The detector gates a mono signal by short-window RMS energy against a
//! threshold, then turns the active spans into padded keep regions over the
//! audio timeline. Removing the complementary dead air is what gives the editor
//! a rough cut instead of a pile of raw takes.
//!
//! It works on raw `&[f32]` samples plus a [`hollywood_timeline::SampleRate`],
//! so it is independent of how the audio was decoded — `hollywood-ffmpeg`'s
//! `MonoAudio` feeds straight in, but a different backend would too. Output is a
//! list of [`hollywood_timeline::TimeRange`] keep regions.

pub mod silence;

mod error;

pub use error::DetectError;
pub use silence::{Dbfs, SilenceGate, keep_regions};
