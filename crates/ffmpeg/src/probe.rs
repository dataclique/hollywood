//! Media probing via FFmpeg, behind a backend-swappable trait.

use std::path::Path;

use ffmpeg_next::codec::context::Context;
use ffmpeg_next::format;
use ffmpeg_next::format::stream::{Disposition, Stream};
use ffmpeg_next::media::Type;
use hollywood_timeline::{
    AudioProperties, ChannelLayout, FrameRate, MediaAsset, MediaSource, SampleRate, Seconds,
    VideoProperties,
};

use crate::error::MediaError;

/// A backend that reads a media source's properties.
///
/// [`FfmpegProbe`] is the default implementation; the trait keeps callers
/// independent of FFmpeg so a pure-Rust backend can replace it.
pub trait MediaProbe {
    /// Read the properties of the media at `path`.
    fn probe(&self, path: &Path) -> Result<ProbedMedia, MediaError>;
}

/// The properties a probe reads from a media source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbedMedia {
    /// Total duration.
    pub duration: Seconds,
    /// Video properties, if the source has a video stream.
    pub video: Option<VideoProperties>,
    /// Audio properties, if the source has an audio stream.
    pub audio: Option<AudioProperties>,
}

impl ProbedMedia {
    /// Build a timeline [`MediaAsset`] for `source` from these properties.
    pub fn into_asset(self, source: MediaSource) -> Result<MediaAsset, MediaError> {
        Ok(MediaAsset::new(
            source,
            self.duration,
            self.video,
            self.audio,
        )?)
    }
}

/// An FFmpeg-backed [`MediaProbe`].
#[derive(Clone, Copy, Debug, Default)]
pub struct FfmpegProbe;

impl MediaProbe for FfmpegProbe {
    fn probe(&self, path: &Path) -> Result<ProbedMedia, MediaError> {
        let input = format::input(path)?;

        // Cover art (an embedded thumbnail on an audio file) is exposed as a
        // still-image video stream; selecting it would mislabel an audio-only
        // file as video, so skip it.
        let video_stream = input
            .streams()
            .best(Type::Video)
            .filter(|stream| !is_attached_picture(stream));
        let audio_stream = input.streams().best(Type::Audio);

        let video_result = video_stream.as_ref().map(probe_video);
        let audio_result = audio_stream.as_ref().map(probe_audio);

        let video = video_result.as_ref().and_then(|r| r.as_ref().ok()).copied();
        let audio = audio_result.as_ref().and_then(|r| r.as_ref().ok()).copied();

        if video.is_none() && audio.is_none() {
            // Nothing decoded. If a stream was present but failed to decode,
            // surface that error rather than reporting no streams — but never
            // discard a usable audio stream because the video side failed.
            if let Some(Err(error)) = video_result {
                return Err(error);
            }
            if let Some(Err(error)) = audio_result {
                return Err(error);
            }
            return Err(MediaError::NoStreams);
        }

        // Prefer the container duration; many valid files (raw/streamed formats,
        // fragmented MP4) carry it only on a stream, so fall back to each in turn
        // — one stream may lack a per-stream duration while another has it.
        let duration = container_seconds(input.duration())
            .or_else(|| video_stream.as_ref().and_then(stream_seconds))
            .or_else(|| audio_stream.as_ref().and_then(stream_seconds))
            .ok_or(MediaError::UnknownDuration)?;

        Ok(ProbedMedia {
            duration,
            video,
            audio,
        })
    }
}

/// FFmpeg's `AVFormatContext.duration` time base — microseconds.
const AV_TIME_BASE: i64 = 1_000_000;

/// The container duration (in [`AV_TIME_BASE`] microseconds) as exact seconds,
/// or `None` if it is non-positive — FFmpeg uses `AV_NOPTS_VALUE` (`i64::MIN`)
/// when the container carries no duration.
fn container_seconds(micros: i64) -> Option<Seconds> {
    if micros <= 0 {
        return None;
    }
    Seconds::new(micros, AV_TIME_BASE).ok()
}

/// A stream's own duration as exact seconds, scaling its raw duration by the
/// stream time base.
fn stream_seconds(stream: &Stream<'_>) -> Option<Seconds> {
    let time_base = stream.time_base();
    raw_stream_seconds(
        stream.duration(),
        i64::from(time_base.numerator()),
        i64::from(time_base.denominator()),
    )
}

/// `raw * (time_base_num / time_base_den)` seconds, or `None` if the duration is
/// non-positive or the time base is degenerate.
fn raw_stream_seconds(raw: i64, time_base_num: i64, time_base_den: i64) -> Option<Seconds> {
    if raw <= 0 || time_base_num <= 0 || time_base_den <= 0 {
        return None;
    }
    let numerator = raw.checked_mul(time_base_num)?;
    Seconds::new(numerator, time_base_den).ok()
}

/// Whether a video stream is merely an embedded still (cover art / thumbnail)
/// rather than real footage.
fn is_attached_picture(stream: &Stream<'_>) -> bool {
    stream.disposition().contains(Disposition::ATTACHED_PIC)
}

fn probe_video(stream: &Stream<'_>) -> Result<VideoProperties, MediaError> {
    let decoder = Context::from_parameters(stream.parameters())?
        .decoder()
        .video()?;

    // FFmpeg reports `avg_frame_rate` as 0/0 for variable-frame-rate or
    // unknown-average streams; fall back to the base rate (`r_frame_rate`).
    let avg = stream.avg_frame_rate();
    let rate = if avg.numerator() > 0 {
        avg
    } else {
        stream.rate()
    };
    let numerator = u32::try_from(rate.numerator()).map_err(|_| MediaError::InvalidFrameRate)?;
    let denominator =
        u32::try_from(rate.denominator()).map_err(|_| MediaError::InvalidFrameRate)?;
    let frame_rate =
        FrameRate::new(numerator, denominator).map_err(|_| MediaError::InvalidFrameRate)?;

    Ok(VideoProperties {
        frame_rate,
        width: decoder.width(),
        height: decoder.height(),
    })
}

fn probe_audio(stream: &Stream<'_>) -> Result<AudioProperties, MediaError> {
    let decoder = Context::from_parameters(stream.parameters())?
        .decoder()
        .audio()?;

    let sample_rate = SampleRate::new(decoder.rate()).map_err(|_| MediaError::InvalidSampleRate)?;

    Ok(AudioProperties {
        sample_rate,
        channels: channel_layout(decoder.channels())?,
    })
}

/// Map a decoder channel count onto the IR layout, deferring the
/// mono/stereo canonicalization to the IR's own constructor. A zero count means
/// the decoder could not read channel info — that is an error, not genuine mono.
fn channel_layout(count: u16) -> Result<ChannelLayout, MediaError> {
    ChannelLayout::channels(count).map_err(|_| MediaError::InvalidChannelLayout)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use super::*;

    #[test]
    fn channel_layout_maps_counts() {
        assert_eq!(channel_layout(1).unwrap(), ChannelLayout::Mono);
        assert_eq!(channel_layout(2).unwrap(), ChannelLayout::Stereo);
        assert_eq!(
            channel_layout(6).unwrap(),
            ChannelLayout::Channels(NonZeroU16::new(6).unwrap())
        );
        // A degenerate zero-channel report is an error, not a silent mono fallback.
        assert!(channel_layout(0).is_err());
    }

    #[test]
    fn container_duration_converts_microseconds() {
        assert_eq!(container_seconds(2_000_000), Some(Seconds::from_secs(2)));
        assert_eq!(
            container_seconds(500_000),
            Some(Seconds::new(1, 2).unwrap())
        );
    }

    #[test]
    fn non_positive_container_duration_is_unknown() {
        // 0 and AV_NOPTS_VALUE (i64::MIN) both mean "no container duration".
        assert_eq!(container_seconds(0), None);
        assert_eq!(container_seconds(-5), None);
        assert_eq!(container_seconds(i64::MIN), None);
    }

    #[test]
    fn stream_duration_scales_by_time_base() {
        // 150 ticks at a 1/30 time base is exactly 5 seconds.
        assert_eq!(raw_stream_seconds(150, 1, 30), Some(Seconds::from_secs(5)));
    }

    #[test]
    fn degenerate_stream_duration_is_unknown() {
        assert_eq!(raw_stream_seconds(0, 1, 30), None);
        assert_eq!(raw_stream_seconds(-1, 1, 30), None);
        assert_eq!(raw_stream_seconds(150, 0, 30), None);
        assert_eq!(raw_stream_seconds(150, 1, 0), None);
    }

    #[test]
    fn overflowing_stream_duration_is_unknown() {
        // raw * time_base_num overflows i64, so the checked multiply yields None
        // rather than wrapping into a plausible-but-wrong duration.
        assert_eq!(raw_stream_seconds(i64::MAX, 2, 1), None);
    }

    #[test]
    fn probed_media_converts_to_asset() {
        let probed = ProbedMedia {
            duration: Seconds::from_secs(10),
            video: None,
            audio: Some(AudioProperties {
                sample_rate: SampleRate::new(48_000).unwrap(),
                channels: ChannelLayout::Stereo,
            }),
        };
        let asset = probed.into_asset(MediaSource::file("a.wav")).unwrap();
        assert_eq!(asset.duration(), Seconds::from_secs(10));
        assert_eq!(
            asset.audio().map(|a| a.channels),
            Some(ChannelLayout::Stereo)
        );
    }

    #[test]
    fn into_asset_rejects_zero_duration() {
        // A zero-duration probe cannot become a valid asset: `MediaAsset::new`
        // rejects a non-positive duration, so `into_asset` surfaces that error.
        // The primary "no streams" guard still lives in `FfmpegProbe::probe`.
        let probed = ProbedMedia {
            duration: Seconds::ZERO,
            video: None,
            audio: None,
        };
        assert!(probed.into_asset(MediaSource::file("x")).is_err());
    }
}
