//! The `process` CLI command: run the whole pipeline over one source file and
//! write the exported NLE documents to disk.
//!
//! This is the headless surface over `hollywood-pipeline`'s
//! [`run`](hollywood_pipeline::run): probe and decode the input, trim dead air,
//! assemble a rough cut, and export it. The FFmpeg backend is wired here; the
//! core [`process`] is generic over the media traits so it is exercised in tests
//! with fakes rather than real media.

use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use thiserror::Error;

use hollywood_detect::{Dbfs, DetectError, SilenceGate};
use hollywood_ffmpeg::{DecodeAudio, FfmpegDecoder, FfmpegProbe, MediaProbe};
use hollywood_pipeline::{ExportTarget, FlowConfig, PipelineError, ProgressReporter, run};
use hollywood_timeline::{FrameRate, Seconds, TimelineError};

/// Arguments to the `process` command.
#[derive(Debug, Args)]
pub struct ProcessArgs {
    /// The source media file to pre-edit.
    input: PathBuf,
    /// Directory to write the exported NLE files into.
    output: PathBuf,
    /// Output sequence frame rate, in whole frames per second. Required: it is
    /// the editorial timebase, not derived from the source.
    #[arg(long)]
    fps: u32,
    /// NLE formats to export (repeat for several).
    #[arg(long = "target", value_enum, default_values_t = [CliTarget::Xmeml])]
    targets: Vec<CliTarget>,
    /// Silence threshold in dBFS; windows quieter than this are treated as dead
    /// air.
    #[arg(long, default_value_t = -40.0)]
    silence_threshold: f32,
    /// Analysis window length, in milliseconds.
    #[arg(long, default_value_t = 20)]
    window_ms: u32,
    /// Padding kept around each kept region, in milliseconds.
    #[arg(long, default_value_t = 50)]
    padding_ms: u32,
}

/// Run the `process` command: build the run configuration, run the pipeline with
/// the FFmpeg backend, and report the files written.
///
/// # Errors
///
/// [`CliError`] if the arguments are invalid, the pipeline fails, or an export
/// file cannot be written.
pub fn run_process(args: &ProcessArgs) -> Result<(), CliError> {
    let config = args.config()?;
    let written = process(
        &FfmpegProbe,
        &FfmpegDecoder,
        &args.input,
        &args.output,
        &config,
    )?;
    for path in written {
        println!("wrote {}", path.display());
    }
    Ok(())
}

/// An NLE export format selectable on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliTarget {
    Xmeml,
    Fcpxml,
    Otio,
}

impl From<CliTarget> for ExportTarget {
    fn from(target: CliTarget) -> Self {
        match target {
            CliTarget::Xmeml => Self::Xmeml,
            CliTarget::Fcpxml => Self::Fcpxml,
            CliTarget::Otio => Self::Otio,
        }
    }
}

impl ProcessArgs {
    /// Build the typed [`FlowConfig`] from the raw arguments, naming the sequence
    /// after the input file.
    fn config(&self) -> Result<FlowConfig, CliError> {
        let name = self
            .input
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or(CliError::BadInputPath)?
            .to_owned();
        let gate = SilenceGate::new(
            Seconds::new(i64::from(self.window_ms), 1_000)?,
            Dbfs::new(self.silence_threshold),
            Seconds::new(i64::from(self.padding_ms), 1_000)?,
        )?;
        Ok(FlowConfig {
            name,
            gate,
            frame_rate: FrameRate::whole(self.fps)?,
            targets: self
                .targets
                .iter()
                .copied()
                .map(ExportTarget::from)
                .collect(),
        })
    }
}

/// Run the pipeline over `input` and write each exported document to `output` as
/// `<sequence name>.<ext>`, returning the paths written. Generic over the media
/// backend so it can be tested without real media.
fn process<P, D>(
    probe: &P,
    decoder: &D,
    input: &Path,
    output: &Path,
    config: &FlowConfig,
) -> Result<Vec<PathBuf>, CliError>
where
    P: MediaProbe,
    D: DecodeAudio,
{
    let reporter = ProgressReporter::new();
    let exported = run(probe, decoder, input, config, &reporter)?;
    std::fs::create_dir_all(output)?;
    exported
        .documents()
        .iter()
        .map(|(target, document)| {
            let path = output.join(format!("{}.{}", config.name, extension(*target)));
            std::fs::write(&path, document)?;
            Ok(path)
        })
        .collect()
}

/// The file extension for an export format.
fn extension(target: ExportTarget) -> &'static str {
    match target {
        ExportTarget::Xmeml => "xml",
        ExportTarget::Fcpxml => "fcpxml",
        ExportTarget::Otio => "otio",
    }
}

/// A failure running the `process` command.
#[derive(Debug, Error)]
pub enum CliError {
    /// The input path has no usable (UTF-8) file name to name the sequence after.
    #[error("the input path has no usable file name")]
    BadInputPath,
    /// An argument did not make a valid timeline value (e.g. a zero frame rate).
    #[error("invalid configuration: {0}")]
    Config(#[from] TimelineError),
    /// The silence-gate arguments were invalid.
    #[error("invalid silence gate: {0}")]
    Gate(#[from] DetectError),
    /// The pipeline run failed.
    #[error("the pipeline failed: {0}")]
    Pipeline(#[from] PipelineError),
    /// Writing an export file failed.
    #[error("writing the export failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use hollywood_ffmpeg::{MediaError, MonoAudio, ProbedMedia};
    use hollywood_timeline::{AudioProperties, ChannelLayout, SampleRate};

    const RATE_HZ: u32 = 48_000;
    const FPS: u32 = 30;
    const FRAME_SAMPLES: usize = RATE_HZ as usize / FPS as usize;

    fn rate() -> SampleRate {
        SampleRate::new(RATE_HZ).unwrap()
    }

    struct FakeProbe;
    impl MediaProbe for FakeProbe {
        fn probe(&self, _path: &Path) -> Result<ProbedMedia, MediaError> {
            Ok(ProbedMedia {
                // A deliberately-bogus duration: decode_source derives the real one
                // from the samples, so the pipeline must ignore this.
                duration: Seconds::from_secs(999),
                video: None,
                audio: Some(AudioProperties {
                    sample_rate: rate(),
                    channels: ChannelLayout::Mono,
                }),
            })
        }
    }

    struct FakeDecoder;
    impl DecodeAudio for FakeDecoder {
        fn decode_mono(&self, _path: &Path) -> Result<MonoAudio, MediaError> {
            // 15 whole frames (frame-aligned duration) with tone over two frame
            // spans, so the assembled clips export cleanly.
            let samples = (0..15 * FRAME_SAMPLES)
                .map(|sample| {
                    let frame = sample / FRAME_SAMPLES;
                    if (3..6).contains(&frame) || (9..12).contains(&frame) {
                        0.8
                    } else {
                        0.0
                    }
                })
                .collect();
            Ok(MonoAudio {
                samples,
                sample_rate: rate(),
            })
        }
    }

    fn config(targets: Vec<ExportTarget>) -> FlowConfig {
        FlowConfig {
            name: "take".to_owned(),
            gate: SilenceGate::new(
                Seconds::new(1, i64::from(FPS)).unwrap(),
                Dbfs::new(-40.0),
                Seconds::ZERO,
            )
            .unwrap(),
            frame_rate: FrameRate::whole(FPS).unwrap(),
            targets,
        }
    }

    #[test]
    fn process_writes_one_file_per_target_named_after_the_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(vec![
            ExportTarget::Xmeml,
            ExportTarget::Fcpxml,
            ExportTarget::Otio,
        ]);

        let written = process(
            &FakeProbe,
            &FakeDecoder,
            Path::new("take.mov"),
            dir.path(),
            &config,
        )
        .unwrap();

        assert_eq!(
            written,
            vec![
                dir.path().join("take.xml"),
                dir.path().join("take.fcpxml"),
                dir.path().join("take.otio"),
            ]
        );
        let xml = std::fs::read_to_string(dir.path().join("take.xml")).unwrap();
        assert_eq!(xml.matches("<clipitem").count(), 2);
    }

    #[test]
    fn config_rejects_a_zero_frame_rate() {
        let args = ProcessArgs {
            input: PathBuf::from("take.mov"),
            output: PathBuf::from("out"),
            fps: 0,
            targets: vec![CliTarget::Xmeml],
            silence_threshold: -40.0,
            window_ms: 20,
            padding_ms: 50,
        };

        assert!(matches!(args.config(), Err(CliError::Config(_))));
    }

    #[test]
    fn a_pipeline_failure_surfaces_as_a_cli_pipeline_error() {
        // A decoder yielding a non-finite sample fails the detector; process must
        // surface that as CliError::Pipeline, not panic or a bare IO error.
        struct NanDecoder;
        impl DecodeAudio for NanDecoder {
            fn decode_mono(&self, _path: &Path) -> Result<MonoAudio, MediaError> {
                Ok(MonoAudio {
                    samples: vec![0.0, f32::NAN, 0.0],
                    sample_rate: rate(),
                })
            }
        }
        let dir = tempfile::tempdir().unwrap();

        let result = process(
            &FakeProbe,
            &NanDecoder,
            Path::new("take.mov"),
            dir.path(),
            &config(vec![ExportTarget::Xmeml]),
        );

        assert!(matches!(result, Err(CliError::Pipeline(_))));
    }

    #[test]
    fn config_names_the_sequence_after_the_input_and_maps_targets() {
        let args = ProcessArgs {
            input: PathBuf::from("/clips/interview.mov"),
            output: PathBuf::from("/out"),
            fps: 24,
            targets: vec![CliTarget::Fcpxml],
            silence_threshold: -40.0,
            window_ms: 20,
            padding_ms: 50,
        };

        let config = args.config().unwrap();

        assert_eq!(config.name, "interview");
        assert_eq!(config.targets, vec![ExportTarget::Fcpxml]);
        assert_eq!(config.frame_rate, FrameRate::whole(24).unwrap());
    }
}
