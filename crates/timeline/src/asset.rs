//! Media assets — the source files a timeline's clips reference.

use std::fmt;
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};

use crate::error::TimelineError;
use crate::time::{FrameRate, SampleRate, Seconds};

/// Where a media asset's bytes come from — and, since the same file is the same
/// asset, its identity within a timeline.
///
/// Modeled as an enum so a new kind of source (a remote URL, a capture device)
/// becomes a new variant rather than another meaning smuggled into a string.
/// File paths are lexically normalized at construction so that `a.mov` and
/// `./a.mov` denote the same asset. [`Display`] produces the on-the-wire path;
/// [`file_name`](Self::file_name) extracts the leaf for labels.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MediaSource {
    /// A local file on disk.
    File(PathBuf),
}

impl MediaSource {
    /// A source backed by a local file. The path is lexically normalized
    /// (redundant `.` and separator components collapsed) so distinct spellings
    /// of the same location share one asset identity.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::File(lexically_normalize(&path.into()))
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

/// Drop redundant `.` and separator components from a path without touching the
/// filesystem, so distinct spellings of the same location (`a.mov`, `./a.mov`,
/// `a/./b`) compare equal. `..` is preserved verbatim — resolving it lexically
/// would be wrong across symlinks.
fn lexically_normalize(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            other => normalized.push(other),
        }
    }
    if normalized.as_os_str().is_empty() {
        normalized.push(Component::CurDir);
    }
    normalized
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
    /// A layout from a channel count. Errors on zero; one and two canonicalize
    /// to [`Mono`](Self::Mono) and [`Stereo`](Self::Stereo) so a layout has a
    /// single representation and equality stays meaningful.
    pub fn channels(count: u16) -> Result<Self, TimelineError> {
        // Reject zero once up front; everything past this point is non-zero, so
        // there is no second failure path to model.
        let count = NonZeroU16::new(count).ok_or(TimelineError::ZeroChannelCount)?;
        Ok(match count.get() {
            1 => Self::Mono,
            2 => Self::Stereo,
            _ => Self::Channels(count),
        })
    }

    /// The number of channels this layout carries — always non-zero, so callers
    /// doing per-channel arithmetic never have to re-establish it.
    pub fn count(self) -> NonZeroU16 {
        match self {
            Self::Mono => NonZeroU16::MIN,
            Self::Stereo => NonZeroU16::MIN.saturating_add(1),
            Self::Channels(n) => n,
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
    /// A media asset with the given probed properties. Errors on a zero or
    /// negative duration — a real source file always occupies time, and a
    /// zero-duration asset cannot be probed or relinked in an NLE — and on an
    /// asset that carries neither a video nor an audio stream.
    pub fn new(
        source: MediaSource,
        duration: Seconds,
        video: Option<VideoProperties>,
        audio: Option<AudioProperties>,
    ) -> Result<Self, TimelineError> {
        if duration.is_negative() || duration.is_zero() {
            return Err(TimelineError::NonPositiveDuration);
        }
        if video.is_none() && audio.is_none() {
            return Err(TimelineError::AssetWithoutStreams);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo() -> AudioProperties {
        AudioProperties {
            sample_rate: SampleRate::new(48_000).unwrap(),
            channels: ChannelLayout::Stereo,
        }
    }

    fn asset(duration: Seconds) -> Result<MediaAsset, TimelineError> {
        MediaAsset::new(MediaSource::file("a.mov"), duration, None, Some(stereo()))
    }

    #[test]
    fn asset_with_a_stream_is_accepted() {
        let media = asset(Seconds::from_secs(10)).unwrap();
        assert_eq!(media.audio(), Some(stereo()));
    }

    #[test]
    fn asset_without_any_stream_is_rejected() {
        assert_eq!(
            MediaAsset::new(
                MediaSource::file("a.mov"),
                Seconds::from_secs(10),
                None,
                None
            ),
            Err(TimelineError::AssetWithoutStreams)
        );
    }

    #[test]
    fn zero_duration_asset_is_rejected() {
        assert_eq!(
            asset(Seconds::ZERO),
            Err(TimelineError::NonPositiveDuration)
        );
    }

    #[test]
    fn negative_duration_asset_is_rejected() {
        let negative = Seconds::new(-1, 1).unwrap();
        assert_eq!(asset(negative), Err(TimelineError::NonPositiveDuration));
    }

    #[test]
    fn channels_rejects_zero() {
        assert_eq!(
            ChannelLayout::channels(0),
            Err(TimelineError::ZeroChannelCount)
        );
    }

    #[test]
    fn channel_count_is_non_zero_for_every_layout() {
        assert_eq!(ChannelLayout::Mono.count().get(), 1);
        assert_eq!(ChannelLayout::Stereo.count().get(), 2);
        assert_eq!(ChannelLayout::channels(6).unwrap().count().get(), 6);
    }

    #[test]
    fn channels_canonicalizes_mono_and_stereo() {
        assert_eq!(ChannelLayout::channels(1).unwrap(), ChannelLayout::Mono);
        assert_eq!(ChannelLayout::channels(2).unwrap(), ChannelLayout::Stereo);
        assert_eq!(
            ChannelLayout::channels(6).unwrap(),
            ChannelLayout::Channels(NonZeroU16::new(6).unwrap())
        );
    }

    #[test]
    fn file_name_returns_the_leaf() {
        assert_eq!(
            MediaSource::file("/footage/day1/a001.mov").file_name(),
            Some("a001.mov")
        );
    }

    #[test]
    fn file_name_is_none_without_a_leaf() {
        assert_eq!(MediaSource::file("/").file_name(), None);
    }

    #[test]
    fn file_identity_ignores_redundant_path_segments() {
        assert_eq!(MediaSource::file("./a.mov"), MediaSource::file("a.mov"));
        assert_eq!(MediaSource::file("a/./b.mov"), MediaSource::file("a/b.mov"));
        assert_eq!(MediaSource::file("a//b.mov"), MediaSource::file("a/b.mov"));
    }
}
