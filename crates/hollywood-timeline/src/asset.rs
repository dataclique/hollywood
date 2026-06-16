//! Media assets — the source files a timeline's clips reference.

use std::fmt;
use std::num::NonZeroU16;

use crate::error::TimelineError;
use crate::time::{FrameRate, SampleRate, Seconds};

/// Stable identity of a media asset within a timeline. Clips relink to assets
/// by this id, so it must be unique within a timeline.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetId(String);

impl AssetId {
    /// Wrap a string as an asset id.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
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

/// A source media file a timeline can place clips from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaAsset {
    id: AssetId,
    duration: Seconds,
    video: Option<VideoProperties>,
    audio: Option<AudioProperties>,
}

impl MediaAsset {
    /// A media asset with the given probed properties. Errors on a negative
    /// duration.
    pub fn new(
        id: AssetId,
        duration: Seconds,
        video: Option<VideoProperties>,
        audio: Option<AudioProperties>,
    ) -> Result<Self, TimelineError> {
        if duration.is_negative() {
            return Err(TimelineError::NegativeDuration);
        }
        Ok(Self {
            id,
            duration,
            video,
            audio,
        })
    }

    /// This asset's id.
    pub fn id(&self) -> &AssetId {
        &self.id
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
