//! Footage the user added and the probe result for each file.

use std::fmt;

use hollywood_ffmpeg::ProbedMedia;
use hollywood_timeline::MediaSource;

/// Probe outcome for one piece of footage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProbeOutcome {
    /// Probe is running on a worker thread.
    Pending,
    /// Properties read successfully.
    Ready(ProbedMedia),
    /// FFmpeg or the IR rejected the file.
    Failed(String),
}

impl ProbeOutcome {
    /// One-line summary for the footage list.
    pub(crate) fn summary(&self) -> String {
        match self {
            Self::Pending => "probing…".to_owned(),
            Self::Ready(media) => {
                let labels: Vec<&str> = [
                    media.video.is_some().then_some("video"),
                    media.audio.is_some().then_some("audio"),
                ]
                .into_iter()
                .flatten()
                .collect();
                let streams = if labels.is_empty() {
                    "no streams".to_owned()
                } else {
                    labels.join(" + ")
                };
                format!("{streams}, {}", format_duration(media.duration))
            }
            Self::Failed(message) => message.clone(),
        }
    }
}

/// One user-selected source file and its probe state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FootageEntry {
    source: MediaSource,
    outcome: ProbeOutcome,
}

impl FootageEntry {
    /// Footage waiting to be probed.
    pub(crate) fn pending(source: MediaSource) -> Self {
        Self {
            source,
            outcome: ProbeOutcome::Pending,
        }
    }

    /// Footage with a finished probe.
    pub(crate) fn probed(source: MediaSource, outcome: ProbeOutcome) -> Self {
        Self { source, outcome }
    }

    pub(crate) fn source(&self) -> &MediaSource {
        &self.source
    }

    pub(crate) fn outcome(&self) -> &ProbeOutcome {
        &self.outcome
    }

    pub(crate) fn label(&self) -> String {
        self.source
            .file_name()
            .map_or_else(|| self.source.to_string(), str::to_owned)
    }
}

fn format_duration(duration: hollywood_timeline::Seconds) -> String {
    let total = duration.as_secs_f64();
    let minutes = (total / 60.0).floor();
    let seconds = total % 60.0;
    format!("{minutes:.0}:{seconds:02.0}")
}

impl fmt::Display for FootageEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} — {}", self.label(), self.outcome.summary())
    }
}

#[cfg(test)]
mod tests {
    use hollywood_ffmpeg::ProbedMedia;
    use hollywood_timeline::{
        AudioProperties, ChannelLayout, FrameRate, SampleRate, Seconds, VideoProperties,
    };

    use super::*;

    fn audio() -> AudioProperties {
        AudioProperties {
            sample_rate: SampleRate::new(48_000).unwrap(),
            channels: ChannelLayout::Stereo,
        }
    }

    fn video() -> VideoProperties {
        VideoProperties {
            frame_rate: FrameRate::new(30, 1).unwrap(),
            width: 1920,
            height: 1080,
        }
    }

    fn ready(
        duration_secs: i64,
        video: Option<VideoProperties>,
        audio: Option<AudioProperties>,
    ) -> ProbeOutcome {
        ProbeOutcome::Ready(ProbedMedia {
            duration: Seconds::from_secs(duration_secs),
            video,
            audio,
        })
    }

    #[test]
    fn pending_summary() {
        assert_eq!(ProbeOutcome::Pending.summary(), "probing…");
    }

    #[test]
    fn failed_summary_is_the_message() {
        assert_eq!(
            ProbeOutcome::Failed("bad file".to_owned()).summary(),
            "bad file"
        );
    }

    #[test]
    fn ready_summary_lists_present_streams() {
        assert_eq!(
            ready(83, Some(video()), Some(audio())).summary(),
            "video + audio, 1:23"
        );
        assert_eq!(ready(5, None, Some(audio())).summary(), "audio, 0:05");
        assert_eq!(ready(600, Some(video()), None).summary(), "video, 10:00");
        assert_eq!(ready(1, None, None).summary(), "no streams, 0:01");
    }

    #[test]
    fn format_duration_is_minutes_and_padded_seconds() {
        assert_eq!(format_duration(Seconds::from_secs(0)), "0:00");
        assert_eq!(format_duration(Seconds::from_secs(9)), "0:09");
        assert_eq!(format_duration(Seconds::from_secs(83)), "1:23");
        assert_eq!(format_duration(Seconds::from_secs(600)), "10:00");
    }

    #[test]
    fn label_is_the_leaf_file_name() {
        let entry = FootageEntry::pending(MediaSource::file("clips/take 1.mov"));
        assert_eq!(entry.label(), "take 1.mov");
    }

    #[test]
    fn display_joins_label_and_summary() {
        let entry = FootageEntry::pending(MediaSource::file("a/clip.mp4"));
        assert_eq!(entry.to_string(), "clip.mp4 — probing…");
    }
}
