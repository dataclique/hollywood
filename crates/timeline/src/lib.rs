//! Hollywood's timeline intermediate representation (IR).
//!
//! This crate is the shared vocabulary every other Hollywood crate speaks: an
//! in-memory model of a multi-track timeline. Invariants are enforced at two
//! levels:
//!
//! - **At construction** — time is exact rational seconds; rates, gaps, and
//!   transitions must be positive; an asset's duration must be positive; a
//!   transition must follow a clip.
//! - **At [`Timeline::validate`]** — cross-references that a single value
//!   cannot check in isolation: every clip points at a registered asset, no
//!   clip's source range escapes its asset's bounds, and every transition is
//!   flanked by clips on both sides.
//!
//! Construction alone does not guarantee a coherent timeline (a clip can be
//! built against an out-of-bounds range, a transition can dangle without a
//! following clip): always call [`Timeline::validate`] before handing the IR to
//! an exporter.
//!
//! The NLE exporters, the assembler, and the pipeline all project to and from
//! these types; see [`SPEC.md`](https://github.com/dataclique/hollywood/blob/master/SPEC.md).
//!
//! ```
//! use hollywood_timeline::{
//!     Clip, FrameRate, MediaAsset, MediaSource, Seconds, TimeRange, Timeline, Track, TrackKind,
//!     VideoProperties,
//! };
//!
//! let fps = FrameRate::whole(30).unwrap();
//! let mut timeline = Timeline::new("demo", fps);
//! let asset = MediaSource::file("a.mov");
//! let video = VideoProperties { frame_rate: fps, width: 1920, height: 1080 };
//! timeline
//!     .add_asset(MediaAsset::new(asset.clone(), Seconds::from_secs(10), Some(video), None).unwrap())
//!     .unwrap();
//!
//! let mut track = Track::new(TrackKind::Video);
//! let range = TimeRange::new(Seconds::from_secs(1), Seconds::from_secs(4)).unwrap();
//! track.push_clip(Clip::new(asset, range));
//! timeline.add_track(track);
//!
//! timeline.validate().unwrap();
//! ```

pub mod asset;
pub mod time;
pub mod timeline;
pub mod track;

mod error;

pub use asset::{AudioProperties, ChannelLayout, MediaAsset, MediaSource, VideoProperties};
pub use error::TimelineError;
pub use time::{FrameRate, SampleRate, Seconds, TimeRange};
pub use timeline::{Timeline, TrackIndex};
pub use track::{Clip, Gap, Track, TrackItem, TrackKind, Transition};
