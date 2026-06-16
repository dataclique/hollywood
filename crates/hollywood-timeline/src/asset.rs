//! Media assets — the source files a timeline's clips reference.

use std::fmt;
use std::num::NonZeroU16;
use std::path::PathBuf;

use crate::error::TimelineError;
use crate::time::{FrameRate, SampleRate, Seconds};

/// Where a media asset's bytes come from — and, since the same file is the same
/// asset, its identity within a timeline.
///
/// Modeled as an enum so a new kind of source (a remote URL, a capture device)
/// becomes a new variant rather than another meaning smuggled into a string.
/// [`Display`] produces the on-the-wire path; [`file_name`](Self::file_name)
/// extracts the leaf for labels.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MediaSource {
    /// A local file on disk.
    File(PathBuf),
}

impl MediaSource {
    /// A source backed by a local file.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::File(path.into())
    }

    /// The leaf file name, if the path has one — used as a clip/relink label.
    pub fn file_name(&self) -> Option<&str> {
        match self {
            Self::File(path) => path.file_name().and_then(|name| name.to_str()),
        }
    }
}

impl fmt::Display for MediaSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(path) => write!(f, "{}", path.display()),
        }
    }
}

/// How many audio channels a source carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelLayout {
    /// One channel.
    Mono,
    /// Two channels.
    Stereo,
    /// An explicit channel count for layouts beyond mono/stereo.
    Channels(NonZeroU16),
}

impl ChannelLayout {
    /// A layout with an explicit channel count. Errors on zero.
    pub fn channels(count: u16) -> Result<Self, TimelineError> {
        NonZeroU16::new(count)
            .map(Self::Channels)
            .ok_or(TimelineError::ZeroChannelCount)
    }

    /// The number of channels this layout carries.
    pub fn count(self) -> u16 {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
            Self::Channels(n) => n.get(),
        }
    }
}

/// Video properties of a media asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoProperties {
    /// Frames per second.
    pub frame_rate: FrameRate,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
}

/// Audio properties of a media asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioProperties {
    /// Samples per second.
    pub sample_rate: SampleRate,
    /// Channel layout.
    pub channels: ChannelLayout,
}

/// A source media file a timeline can place clips from, identified by its
/// [`MediaSource`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaAsset {
    source: MediaSource,
    duration: Seconds,
    video: Option<VideoProperties>,
    audio: Option<AudioProperties>,
}

impl MediaAsset {
    /// A media asset with the given probed properties. Errors on a negative
    /// duration.
    pub fn new(
        source: MediaSource,
        duration: Seconds,
        video: Option<VideoProperties>,
        audio: Option<AudioProperties>,
    ) -> Result<Self, TimelineError> {
        if duration.is_negative() {
            return Err(TimelineError::NegativeDuration);
        }
        Ok(Self {
            source,
            duration,
            video,
            audio,
        })
    }

    /// This asset's source — its identity.
    pub fn source(&self) -> &MediaSource {
        &self.source
    }

    /// This asset's total duration.
    pub fn duration(&self) -> Seconds {
        self.duration
    }

    /// Video properties, if the asset has a video stream.
    pub fn video(&self) -> Option<VideoProperties> {
        self.video
    }

    /// Audio properties, if the asset has an audio stream.
    pub fn audio(&self) -> Option<AudioProperties> {
        self.audio
    }
}
