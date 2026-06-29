//! The pipeline's probe/decode front: turn a source file into the flow's
//! [`Decoded`] entry state (the `Sources → Probed → Decoded` head of ADR 0008).
//!
//! [`decode_source`] probes and decodes one file behind the backend-swappable
//! [`MediaProbe`]/[`DecodeAudio`] traits, building a [`Decoded`] whose asset
//! duration is the decoded sample span — so the [`Decoded`] invariant holds by
//! construction rather than relying on the probed container duration, which can
//! disagree with the decoded samples. [`run`] chains it onto [`run_flow`] for the
//! whole pipeline: probe/decode → detect → sync → assemble → export.
//!
//! Single source; multi-source (several files aligned by sync) is a follow-up.

use std::path::Path;

use thiserror::Error;

use hollywood_ffmpeg::{DecodeAudio, MediaError, MediaProbe, MonoAudio};
use hollywood_timeline::{MediaAsset, MediaSource, Seconds, TimelineError};

use crate::error::PipelineError;
use crate::flow::{Decoded, Exported, FlowConfig, FlowError, run_flow, wrap};
use crate::progress::ProgressReporter;
use crate::stage::PipelineStage;

/// Run the whole pipeline over one source file: probe and decode it, then trim,
/// sync, assemble, and export via [`run_flow`].
///
/// `probe` and `decoder` are the media backend (e.g. `FfmpegProbe` /
/// `FfmpegDecoder`); the traits keep this independent of FFmpeg. Progress is
/// reported over `reporter`, starting at [`PipelineStage::Probe`] (which covers
/// both probe and decode).
///
/// # Errors
///
/// [`PipelineError::Stage`] for the first stage that fails; the probe/decode
/// front reports as [`PipelineStage::Probe`].
pub fn run<P, D>(
    probe: &P,
    decoder: &D,
    path: &Path,
    config: &FlowConfig,
    reporter: &ProgressReporter,
) -> Result<Exported, PipelineError>
where
    P: MediaProbe,
    D: DecodeAudio,
{
    reporter.enter(PipelineStage::Probe);
    let entry = wrap(
        reporter,
        PipelineStage::Probe,
        decode_source(probe, decoder, path),
    )?;
    run_flow(entry, config, reporter)
}

/// Probe and decode the source at `path` into a [`Decoded`] state, with its asset
/// duration set from the decoded sample count (not the probed container
/// duration), so the [`Decoded`] invariant holds.
///
/// # Errors
///
/// [`SourceError`] if probing, decoding, or building the asset fails, or the
/// decoded sample count is unrepresentable.
pub fn decode_source<P, D>(probe: &P, decoder: &D, path: &Path) -> Result<Decoded, SourceError>
where
    P: MediaProbe,
    D: DecodeAudio,
{
    let properties = probe.probe(path)?;
    let audio = decoder.decode_mono(path)?;
    let duration = audio_span(&audio)?;
    let asset = MediaAsset::new(
        MediaSource::file(path),
        duration,
        properties.video,
        properties.audio,
    )?;
    // The asset duration is `audio_span` over these same samples and rate, so
    // this satisfies the `Decoded` invariant by construction; the re-check only
    // guards against future drift between here and `Decoded::new`.
    let entry = Decoded::new(asset, audio.samples, audio.sample_rate)?;
    Ok(entry)
}

/// An error producing a [`Decoded`] state from a source file.
#[derive(Debug, Error)]
pub enum SourceError {
    /// Probing or decoding the source failed.
    #[error("probing or decoding the source failed")]
    Media(#[from] MediaError),
    /// The decoded sample count does not fit in `i64`.
    #[error("decoded sample count is too large to represent")]
    SampleCount(#[from] std::num::TryFromIntError),
    /// The probed properties did not make a valid media asset.
    #[error("building the source's media asset failed")]
    Asset(#[from] TimelineError),
    /// The decoded source is internally inconsistent.
    #[error("the decoded source is inconsistent")]
    Decoded(#[from] FlowError),
}

/// The duration the decoded `audio` covers, from its sample count.
fn audio_span(audio: &MonoAudio) -> Result<Seconds, SourceError> {
    let count = i64::try_from(audio.samples.len())?;
    Ok(Seconds::from_samples(count, audio.sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExportTarget;
    use crate::progress::RunProgress;
    use hollywood_ffmpeg::ProbedMedia;
    use hollywood_timeline::{AudioProperties, ChannelLayout, FrameRate, SampleRate};

    const RATE_HZ: u32 = 48_000;
    const FPS: u32 = 30;
    const FRAME_SAMPLES: usize = RATE_HZ as usize / FPS as usize;

    fn rate() -> SampleRate {
        SampleRate::new(RATE_HZ).unwrap()
    }

    /// A canned probe returning fixed properties without touching a file.
    struct FakeProbe {
        probed: Result<ProbedMedia, ()>,
    }

    impl MediaProbe for FakeProbe {
        fn probe(&self, _path: &Path) -> Result<ProbedMedia, MediaError> {
            self.probed.map_err(|()| MediaError::NoStreams)
        }
    }

    /// A canned decoder returning fixed samples (or a failure) without a file.
    struct FakeDecoder {
        decoded: Result<Vec<f32>, ()>,
    }

    impl DecodeAudio for FakeDecoder {
        fn decode_mono(&self, _path: &Path) -> Result<MonoAudio, MediaError> {
            let samples = self.decoded.clone().map_err(|()| MediaError::NoAudioData)?;
            Ok(MonoAudio {
                samples,
                sample_rate: rate(),
            })
        }
    }

    fn probed(duration: Seconds) -> ProbedMedia {
        ProbedMedia {
            duration,
            video: None,
            audio: Some(AudioProperties {
                sample_rate: rate(),
                channels: ChannelLayout::Mono,
            }),
        }
    }

    /// `frames` long, with `0.8`-amplitude tone over each `[start, end)` frame
    /// span — frame-aligned so the assembled clips export cleanly.
    fn tone_over_frames(frames: usize, loud: &[(usize, usize)]) -> Vec<f32> {
        (0..frames * FRAME_SAMPLES)
            .map(|sample| {
                let frame = sample / FRAME_SAMPLES;
                let in_tone = loud
                    .iter()
                    .any(|&(start, end)| frame >= start && frame < end);
                if in_tone { 0.8 } else { 0.0 }
            })
            .collect()
    }

    fn config() -> FlowConfig {
        FlowConfig {
            name: "rough cut".to_owned(),
            gate: hollywood_detect::SilenceGate::new(
                Seconds::new(1, i64::from(FPS)).unwrap(),
                hollywood_detect::Dbfs::new(-40.0),
                Seconds::ZERO,
            )
            .unwrap(),
            frame_rate: FrameRate::whole(FPS).unwrap(),
            targets: vec![ExportTarget::Xmeml],
        }
    }

    #[test]
    fn decode_source_uses_the_decoded_span_not_the_probed_duration() {
        // The probe reports a duration that disagrees with the decoded samples (as
        // real container metadata can). decode_source must build the asset from
        // the sample count, so the Decoded invariant holds and construction
        // succeeds despite the bogus probed duration.
        let samples = tone_over_frames(15, &[(3, 6)]);
        let probe = FakeProbe {
            probed: Ok(probed(Seconds::from_secs(999))),
        };
        let decoder = FakeDecoder {
            decoded: Ok(samples),
        };

        assert!(decode_source(&probe, &decoder, Path::new("take.wav")).is_ok());
    }

    #[test]
    fn run_probes_decodes_and_runs_the_whole_pipeline() {
        let probe = FakeProbe {
            probed: Ok(probed(Seconds::from_secs(999))),
        };
        let decoder = FakeDecoder {
            decoded: Ok(tone_over_frames(15, &[(3, 6), (9, 12)])),
        };
        let reporter = ProgressReporter::new();
        let subscription = reporter.subscribe();

        let exported = run(
            &probe,
            &decoder,
            Path::new("take.wav"),
            &config(),
            &reporter,
        )
        .unwrap();

        // The two tone regions reach two clips in the serialized output — built
        // from the decoded samples, not the bogus 999 s probed duration.
        let (_, xmeml) = exported.documents().first().unwrap();
        assert_eq!(xmeml.matches("<clipitem").count(), 2);
        assert_eq!(subscription.current(), RunProgress::Completed);
    }

    #[test]
    fn a_probe_failure_surfaces_as_a_probe_stage_error() {
        let probe = FakeProbe { probed: Err(()) };
        let decoder = FakeDecoder {
            decoded: Ok(tone_over_frames(15, &[(3, 6)])),
        };
        let reporter = ProgressReporter::new();
        let subscription = reporter.subscribe();

        let result = run(
            &probe,
            &decoder,
            Path::new("take.wav"),
            &config(),
            &reporter,
        );

        assert!(matches!(
            result,
            Err(PipelineError::Stage {
                stage: PipelineStage::Probe,
                ..
            })
        ));
        assert_eq!(
            subscription.current(),
            RunProgress::Failed(PipelineStage::Probe)
        );
    }

    #[test]
    fn a_decode_failure_surfaces_as_a_probe_stage_error() {
        // Probe succeeds but decode fails; the front still reports the Probe stage
        // (it covers both probe and decode), not a later one.
        let probe = FakeProbe {
            probed: Ok(probed(Seconds::from_secs(1))),
        };
        let decoder = FakeDecoder { decoded: Err(()) };
        let reporter = ProgressReporter::new();
        let subscription = reporter.subscribe();

        let result = run(
            &probe,
            &decoder,
            Path::new("take.wav"),
            &config(),
            &reporter,
        );

        assert!(matches!(
            result,
            Err(PipelineError::Stage {
                stage: PipelineStage::Probe,
                ..
            })
        ));
        assert_eq!(
            subscription.current(),
            RunProgress::Failed(PipelineStage::Probe)
        );
    }
}
