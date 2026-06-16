//! Hollywood's timeline intermediate representation (IR).
//!
//! This crate is the shared vocabulary every other Hollywood crate speaks: an
//! in-memory model of a multi-track timeline. It is designed so invalid states
//! are unrepresentable — time is exact rational seconds, durations cannot be
//! negative, transitions can only sit between clips, and clips cannot reference
//! a span beyond their asset.
//!
//! The NLE exporters, the assembler, and the pipeline all project to and from
//! these types; see [`SPEC.md`](https://github.com/data-cartel/hollywood/blob/master/SPEC.md).
//!
//! ```
//! use hollywood_timeline::{
//!     AssetId, FrameRate, MediaAsset, Seconds, TimeRange, Timeline, Track, TrackKind, Clip,
//! };
//!
//! let mut timeline = Timeline::new("demo", FrameRate::whole(30).unwrap());
//! timeline
//!     .add_asset(MediaAsset::new(AssetId::new("a"), Seconds::from_secs(10), None, None).unwrap())
//!     .unwrap();
//!
//! let mut track = Track::new(TrackKind::Video);
//! let source = TimeRange::new(Seconds::from_secs(1), Seconds::from_secs(4)).unwrap();
//! track.push_clip(Clip::new(AssetId::new("a"), source));
//! timeline.add_track(track);
//!
//! timeline.validate().unwrap();
//! ```

pub mod asset;
pub mod time;
pub mod timeline;
pub mod track;

mod error;

pub use asset::{AssetId, AudioProperties, ChannelLayout, MediaAsset, VideoProperties};
pub use error::TimelineError;
pub use time::{FrameRate, SampleRate, Seconds, TimeRange};
pub use timeline::Timeline;
pub use track::{Clip, Gap, Track, TrackItem, TrackKind, Transition};
