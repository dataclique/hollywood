//! Footage the user added and the probe result for each file.

use std::fmt;

use hollywood_ffmpeg::ProbedMedia;
use hollywood_timeline::MediaSource;

/// Probe outcome for one piece of footage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// Probe is running on a worker thread.
    Pending,
    /// Properties read successfully.
    Ready(ProbedMedia),
    /// FFmpeg or the IR rejected the file.
    Failed(String),
}

impl ProbeOutcome {
    /// One-line summary for the footage list.
    pub fn summary(&self) -> String {
        match self {
            Self::Pending => "probing…".to_owned(),
            Self::Ready(media) => {
                let mut parts = Vec::new();
                if media.video.is_some() {
                    parts.push("video");
                }
                if media.audio.is_some() {
                    parts.push("audio");
                }
                let streams = if parts.is_empty() {
                    "no streams".to_owned()
                } else {
                    parts.join(" + ")
                };
                format!("{streams}, {}", format_duration(media.duration))
            }
            Self::Failed(message) => message.clone(),
        }
    }
}

/// One user-selected source file and its probe state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FootageEntry {
    source: MediaSource,
    outcome: ProbeOutcome,
}

impl FootageEntry {
    /// Footage waiting to be probed.
    pub fn pending(source: MediaSource) -> Self {
        Self {
            source,
            outcome: ProbeOutcome::Pending,
        }
    }

    /// Footage with a finished probe.
    pub fn probed(source: MediaSource, outcome: ProbeOutcome) -> Self {
        Self { source, outcome }
    }

    pub fn source(&self) -> &MediaSource {
        &self.source
    }

    pub fn outcome(&self) -> &ProbeOutcome {
        &self.outcome
    }

    pub fn label(&self) -> String {
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
