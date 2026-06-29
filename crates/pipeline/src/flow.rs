//! The pipeline's typed-state flow (ADR 0008).
//!
//! A run threads typed state through the stages so an out-of-order run is
//! unrepresentable: each transform consumes the previous state by value and
//! produces the next, and a later stage cannot be written without the earlier
//! one's output. This module wires the in-memory stages — detect → sync →
//! assemble → export — over already-[`Decoded`] audio. [`run_flow`] drives them,
//! reusing the orchestration signals from the rest of the crate: it reports
//! [`RunProgress`](crate::RunProgress) as it enters each stage and wraps a
//! stage's native error in [`PipelineError::Stage`].
//!
//! Scope of this first slice (the rest follows per ADR 0008):
//!
//! - the probe/decode front that produces the [`Decoded`] entry state from a
//!   source file lives in the `source` module; `run_flow` takes `Decoded`
//!   directly;
//! - `sync` is a single-source pass-through; cross-source alignment (via
//!   `align`/`drift_map`) lands with multi-source input;
//! - the keep regions are conformed to whole frames before assembly
//!   ([`conform_to_frames`]), so any analysis window exports cleanly **when the
//!   source duration is itself a whole number of frames**; a samples-derived
//!   source duration that is not frame-aligned is the remaining gap (a region
//!   reaching the source end clamps to that off-frame boundary) — a follow-up.

use itertools::Itertools;
use thiserror::Error;

use hollywood_assemble::assemble;
use hollywood_detect::{SilenceGate, keep_regions};
use hollywood_nle::{to_fcpxml, to_otio, to_xmeml};
use hollywood_timeline::{
    FrameRate, MediaAsset, SampleRate, Seconds, TimeRange, Timeline, TimelineError,
};

use crate::error::PipelineError;
use crate::progress::ProgressReporter;
use crate::stage::PipelineStage;

/// Decoded audio for one source: its probed asset and mono samples. The flow's
/// entry state — the probe/decode front that produces it is a follow-up.
#[derive(Clone, Debug)]
pub struct Decoded {
    asset: MediaAsset,
    samples: Vec<f32>,
    sample_rate: SampleRate,
}

impl Decoded {
    /// Decoded `samples` (mono, in `[-1.0, 1.0]`) at `sample_rate`, paired with
    /// the source's probed `asset`.
    ///
    /// The `asset`'s duration must equal the decoded span
    /// (`samples.len() / sample_rate`), and this enforces it: keep regions are
    /// derived from the samples but validated against the asset when assembled,
    /// so a mismatched duration would reject the tail region at the assemble
    /// stage. The probe/decode front that produces `Decoded` sets the asset
    /// duration from the decoded sample count, so the two agree by construction.
    ///
    /// # Errors
    ///
    /// [`FlowError::DurationMismatch`] if the `asset`'s duration is not the
    /// decoded span, and [`FlowError::SampleCount`] if the sample count exceeds
    /// `i64`.
    pub fn new(
        asset: MediaAsset,
        samples: Vec<f32>,
        sample_rate: SampleRate,
    ) -> Result<Self, FlowError> {
        let span = Seconds::from_samples(i64::try_from(samples.len())?, sample_rate);
        if asset.duration() != span {
            return Err(FlowError::DurationMismatch {
                asset: asset.duration(),
                span,
            });
        }
        Ok(Self {
            asset,
            samples,
            sample_rate,
        })
    }
}

/// An error constructing a [`Decoded`] state.
#[derive(Debug, Error)]
pub enum FlowError {
    /// The decoded sample count does not fit in `i64`.
    #[error("decoded sample count is too large to represent")]
    SampleCount(#[from] std::num::TryFromIntError),
    /// The asset's duration disagrees with the decoded sample span, which would
    /// later reject a keep region at the assemble stage.
    #[error("asset duration {asset:?} does not match the decoded span {span:?}")]
    DurationMismatch {
        /// The probed asset's duration.
        asset: Seconds,
        /// The span the decoded samples actually cover.
        span: Seconds,
    },
}

/// How a run is configured: the analysis and assembly parameters, and which NLE
/// formats to emit. Every value is supplied explicitly — no hidden defaults.
#[derive(Clone, Debug)]
pub struct FlowConfig {
    /// The assembled sequence's name.
    pub name: String,
    /// The silence gate that splits each source into keep regions.
    pub gate: SilenceGate,
    /// The assembled sequence's frame rate (the output timebase, chosen by the
    /// caller — not derived from a source's probed rate).
    pub frame_rate: FrameRate,
    /// The NLE formats to serialize the assembled timeline to.
    pub targets: Vec<ExportTarget>,
}

/// An NLE export format the flow can emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportTarget {
    /// FCP7 xmeml (opens in Premiere and Resolve).
    Xmeml,
    /// FCPXML (Final Cut / Resolve).
    Fcpxml,
    /// OpenTimelineIO JSON.
    Otio,
}

/// A run's output: each requested [`ExportTarget`]'s serialized document, in the
/// order the targets were configured.
#[derive(Clone, Debug)]
pub struct Exported {
    documents: Vec<(ExportTarget, String)>,
}

impl Exported {
    /// The serialized documents, one per requested target.
    pub fn documents(&self) -> &[(ExportTarget, String)] {
        &self.documents
    }
}

/// Run the in-memory flow over one source's [`Decoded`] audio.
///
/// Detects keep regions, syncs (single-source pass-through), assembles the
/// trimmed timeline, and exports it to each configured target. Progress is
/// reported over `reporter` as each stage begins, and the run completes or fails
/// fast.
///
/// # Errors
///
/// [`PipelineError::NoExportTargets`] if `config` has no export targets, and
/// [`PipelineError::Stage`] carrying the stage that failed and its underlying
/// error the first time a stage returns `Err`; later stages do not run.
pub fn run_flow(
    decoded: Decoded,
    config: &FlowConfig,
    reporter: &ProgressReporter,
) -> Result<Exported, PipelineError> {
    if config.targets.is_empty() {
        return Err(PipelineError::NoExportTargets);
    }

    reporter.enter(PipelineStage::Detect);
    let detected = wrap(reporter, PipelineStage::Detect, detect(decoded, config))?;

    reporter.enter(PipelineStage::Sync);
    let synced = sync(detected);

    reporter.enter(PipelineStage::Assemble);
    let assembled = wrap(
        reporter,
        PipelineStage::Assemble,
        assemble_stage(synced, config),
    )?;

    reporter.enter(PipelineStage::Export);
    let exported = wrap(reporter, PipelineStage::Export, export(assembled, config))?;

    reporter.complete();
    Ok(exported)
}

/// One source's audio with the keep regions detected over it.
struct Detected {
    decoded: Decoded,
    regions: Vec<TimeRange>,
}

/// The asset and keep regions ready to assemble, after sync. Single-source sync
/// drops the samples (alignment is done); multi-source will add cross-source
/// offsets here.
struct Synced {
    asset: MediaAsset,
    regions: Vec<TimeRange>,
}

/// The assembled, trimmed timeline ready to export.
struct Assembled {
    timeline: Timeline,
}

fn detect(
    decoded: Decoded,
    config: &FlowConfig,
) -> Result<Detected, hollywood_detect::DetectError> {
    let regions = keep_regions(&decoded.samples, decoded.sample_rate, &config.gate)?;
    Ok(Detected { decoded, regions })
}

fn sync(detected: Detected) -> Synced {
    // One source: nothing to align against, so this cannot fail. Cross-source
    // alignment (over the per-source samples, using a configured method/window)
    // lands with multi-source input — and returns a `Result` once it can fail.
    Synced {
        asset: detected.decoded.asset,
        regions: detected.regions,
    }
}

fn assemble_stage(
    synced: Synced,
    config: &FlowConfig,
) -> Result<Assembled, hollywood_assemble::AssembleError> {
    let source = synced.asset.duration();
    let regions = conform_to_frames(&synced.regions, config.frame_rate, source)?;
    let timeline = assemble(
        config.name.as_str(),
        config.frame_rate,
        synced.asset,
        &regions,
    )?;
    Ok(Assembled { timeline })
}

/// Snap each keep region out to whole `rate` frames — start floored, end ceiled
/// so no speech is clipped — clamped to the `source` duration, then merge any
/// regions that now touch or overlap. The frame-based NLE exporters require clip
/// boundaries on whole frames; this conforms the sample-precise regions to the
/// output frame grid.
///
/// This is exact only when `source` is itself a whole number of frames: a region
/// reaching the source end clamps to `source`, so a samples-derived source
/// duration that is not frame-aligned leaves that one clip boundary off-frame,
/// which every exporter rejects. Conforming the source duration too is a
/// follow-up.
fn conform_to_frames(
    regions: &[TimeRange],
    rate: FrameRate,
    source: Seconds,
) -> Result<Vec<TimeRange>, TimelineError> {
    regions
        .iter()
        .map(|region| {
            let start = Seconds::from_frames(region.start().frame_floor(rate), rate);
            let end = Seconds::from_frames(region.end().frame_ceil(rate), rate).min(source);
            (start, end)
        })
        .coalesce(|(start, end), (next_start, next_end)| {
            // Snapping out can push a region onto its predecessor; fuse the two
            // whenever the next one starts at or before this one ends.
            if next_start <= end {
                Ok((start, end.max(next_end)))
            } else {
                Err(((start, end), (next_start, next_end)))
            }
        })
        .map(|(start, end)| frame_range(start, end))
        .collect()
}

/// The `[start, end)` range, computing the duration without overflow.
fn frame_range(start: Seconds, end: Seconds) -> Result<TimeRange, TimelineError> {
    let duration = end
        .checked_sub(start)
        .ok_or(TimelineError::TimeRangeOverflow)?;
    TimeRange::new(start, duration)
}

fn export(assembled: Assembled, config: &FlowConfig) -> Result<Exported, hollywood_nle::NleError> {
    let Assembled { timeline } = assembled;
    let documents = config
        .targets
        .iter()
        .map(|&target| Ok((target, render(target, &timeline)?)))
        .collect::<Result<Vec<_>, hollywood_nle::NleError>>()?;
    Ok(Exported { documents })
}

fn render(target: ExportTarget, timeline: &Timeline) -> Result<String, hollywood_nle::NleError> {
    match target {
        ExportTarget::Xmeml => to_xmeml(timeline),
        ExportTarget::Fcpxml => to_fcpxml(timeline),
        ExportTarget::Otio => to_otio(timeline),
    }
}

/// Run a stage's `result`, reporting failure and wrapping the native error in
/// [`PipelineError::Stage`] so each stage's distinct error surfaces uniformly.
pub(crate) fn wrap<T, E>(
    reporter: &ProgressReporter,
    stage: PipelineStage,
    result: Result<T, E>,
) -> Result<T, PipelineError>
where
    E: std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(value) => Ok(value),
        Err(source) => {
            reporter.fail(stage);
            Err(PipelineError::Stage {
                stage,
                source: Box::new(source),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::RunProgress;
    use hollywood_detect::Dbfs;
    use hollywood_timeline::{
        AudioProperties, ChannelLayout, MediaSource, Seconds, Track, TrackItem,
    };

    const RATE_HZ: u32 = 48_000;
    const FPS: u32 = 30;
    // One frame at 30fps is 1600 samples at 48kHz; analyzing and padding in whole
    // frames keeps every region boundary on a frame, so the frame-based exporters
    // accept the assembled clips.
    const FRAME_SAMPLES: usize = RATE_HZ as usize / FPS as usize;

    fn rate() -> SampleRate {
        SampleRate::new(RATE_HZ).unwrap()
    }

    /// A gate that analyzes one frame at a time with no padding, so detected
    /// regions land on frame boundaries.
    fn frame_gate() -> SilenceGate {
        SilenceGate::new(
            Seconds::new(1, i64::from(FPS)).unwrap(),
            Dbfs::new(-40.0),
            Seconds::ZERO,
        )
        .unwrap()
    }

    /// `frames` long, with `0.8`-amplitude tone over each `[start, end)` frame
    /// span and silence elsewhere.
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

    /// `len` samples with `0.8`-amplitude tone over each `[start, end)` **sample**
    /// span — for placing tone off the frame grid.
    fn tone_over_samples(len: usize, loud: &[(usize, usize)]) -> Vec<f32> {
        (0..len)
            .map(|sample| {
                let in_tone = loud
                    .iter()
                    .any(|&(start, end)| sample >= start && sample < end);
                if in_tone { 0.8 } else { 0.0 }
            })
            .collect()
    }

    fn audio_asset(samples_len: usize) -> MediaAsset {
        MediaAsset::new(
            MediaSource::file("take.wav"),
            Seconds::from_samples(i64::try_from(samples_len).unwrap(), rate()),
            None,
            Some(AudioProperties {
                sample_rate: rate(),
                channels: ChannelLayout::Mono,
            }),
        )
        .unwrap()
    }

    /// Two tone bursts (frames 3–6 and 9–12) over a 15-frame take, paired with a
    /// matching asset.
    fn two_region_decoded() -> Decoded {
        let samples = tone_over_frames(15, &[(3, 6), (9, 12)]);
        let asset = audio_asset(samples.len());
        Decoded::new(asset, samples, rate()).unwrap()
    }

    fn config(targets: Vec<ExportTarget>) -> FlowConfig {
        FlowConfig {
            name: "rough cut".to_owned(),
            gate: frame_gate(),
            frame_rate: FrameRate::whole(FPS).unwrap(),
            targets,
        }
    }

    fn clip_count(timeline: &Timeline) -> usize {
        timeline
            .tracks()
            .iter()
            .flat_map(Track::items)
            .filter(|item| matches!(item, TrackItem::Clip(_)))
            .count()
    }

    #[test]
    fn detect_splits_the_take_into_its_tone_regions() {
        let detected = detect(two_region_decoded(), &config(vec![])).unwrap();
        assert_eq!(detected.regions.len(), 2);
    }

    #[test]
    fn assemble_lays_one_clip_per_kept_region() {
        let detected = detect(two_region_decoded(), &config(vec![])).unwrap();
        let synced = sync(detected);
        let assembled = assemble_stage(synced, &config(vec![])).unwrap();
        assert_eq!(clip_count(&assembled.timeline), 2);
    }

    #[test]
    fn run_flow_emits_each_requested_target_in_order() {
        let targets = vec![
            ExportTarget::Xmeml,
            ExportTarget::Fcpxml,
            ExportTarget::Otio,
        ];
        let reporter = ProgressReporter::new();
        let exported = run_flow(two_region_decoded(), &config(targets), &reporter).unwrap();

        let emitted: Vec<ExportTarget> = exported.documents().iter().map(|(t, _)| *t).collect();
        assert_eq!(
            emitted,
            vec![
                ExportTarget::Xmeml,
                ExportTarget::Fcpxml,
                ExportTarget::Otio
            ]
        );
        assert!(exported.documents().iter().all(|(_, doc)| !doc.is_empty()));

        // The two kept regions thread all the way to two clips in the serialized
        // xmeml — the output reflects the input, not just a non-empty document.
        // (Exact-XML golden coverage lives in the `hollywood-nle` exporter tests.)
        let xmeml = exported
            .documents()
            .iter()
            .find(|(target, _)| *target == ExportTarget::Xmeml)
            .map(|(_, doc)| doc.as_str())
            .unwrap();
        assert_eq!(xmeml.matches("<clipitem").count(), 2);
    }

    #[test]
    fn run_flow_xmeml_matches_the_golden() {
        // End-to-end: the whole detect → assemble → export chain over the
        // two-region fixture produces exactly the checked-in xmeml document, so a
        // regression in any stage's hand-off or in the serialized contract is
        // caught — not just that the output is non-empty.
        let reporter = ProgressReporter::new();
        let exported = run_flow(
            two_region_decoded(),
            &config(vec![ExportTarget::Xmeml]),
            &reporter,
        )
        .unwrap();
        let (_, xmeml) = exported.documents().first().unwrap();
        assert_golden("flow_two_region.xml", xmeml);
    }

    /// Compare `actual` against the checked-in golden, matching the `hollywood-nle`
    /// harness: `UPDATE_GOLDENS=1` (re)writes it, and a missing golden fails loudly
    /// rather than being silently minted.
    fn assert_golden(name: &str, actual: &str) {
        let path: std::path::PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "golden", name]
            .iter()
            .collect();
        if std::env::var_os("UPDATE_GOLDENS").is_some() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, actual).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&path)
            .expect("golden missing — run with UPDATE_GOLDENS=1 to create it");
        assert_eq!(
            actual, expected,
            "golden {name} mismatch (run UPDATE_GOLDENS=1 to regenerate)"
        );
    }

    #[test]
    fn run_flow_reports_progress_to_completion() {
        let reporter = ProgressReporter::new();
        let subscription = reporter.subscribe();
        run_flow(
            two_region_decoded(),
            &config(vec![ExportTarget::Otio]),
            &reporter,
        )
        .unwrap();
        assert_eq!(subscription.current(), RunProgress::Completed);
    }

    #[test]
    fn a_failing_stage_surfaces_as_pipeline_error_for_that_stage() {
        // A non-finite sample makes the detector fail; the flow must surface it as
        // a Detect-stage failure, not a panic or a later-stage error.
        let mut samples = tone_over_frames(15, &[(3, 6)]);
        if let Some(slot) = samples.get_mut(4 * FRAME_SAMPLES) {
            *slot = f32::NAN;
        }
        let decoded = Decoded::new(audio_asset(samples.len()), samples, rate()).unwrap();

        let reporter = ProgressReporter::new();
        let subscription = reporter.subscribe();
        let result = run_flow(decoded, &config(vec![ExportTarget::Otio]), &reporter);

        assert!(matches!(
            result,
            Err(PipelineError::Stage {
                stage: PipelineStage::Detect,
                ..
            })
        ));
        assert_eq!(
            subscription.current(),
            RunProgress::Failed(PipelineStage::Detect)
        );
    }

    #[test]
    fn an_export_failure_surfaces_as_an_export_stage_error() {
        // A fractional (NTSC) sequence rate assembles fine but no exporter
        // supports it, so the run reaches export and fails there — the flow must
        // label it an Export failure, not mislabel it as an earlier stage.
        let config = FlowConfig {
            name: "rough cut".to_owned(),
            gate: frame_gate(),
            frame_rate: FrameRate::new(30_000, 1_001).unwrap(),
            targets: vec![ExportTarget::Xmeml],
        };
        let reporter = ProgressReporter::new();
        let subscription = reporter.subscribe();
        let result = run_flow(two_region_decoded(), &config, &reporter);

        assert!(matches!(
            result,
            Err(PipelineError::Stage {
                stage: PipelineStage::Export,
                ..
            })
        ));
        assert_eq!(
            subscription.current(),
            RunProgress::Failed(PipelineStage::Export)
        );
    }

    #[test]
    fn an_all_silent_take_runs_to_completion_with_an_empty_cut() {
        // A fully dead-air source yields no keep regions; the flow still assembles
        // an empty timeline and exports it, completing rather than erroring.
        let samples = vec![0.0_f32; 15 * FRAME_SAMPLES];
        let decoded = Decoded::new(audio_asset(samples.len()), samples, rate()).unwrap();
        let reporter = ProgressReporter::new();
        let subscription = reporter.subscribe();

        let exported = run_flow(decoded, &config(vec![ExportTarget::Xmeml]), &reporter).unwrap();

        assert_eq!(subscription.current(), RunProgress::Completed);
        // The cut is genuinely empty — the exported timeline has no clips, not
        // merely a non-empty document.
        let (_, xmeml) = exported.documents().first().unwrap();
        assert_eq!(xmeml.matches("<clipitem").count(), 0);
    }

    #[test]
    fn run_flow_rejects_a_run_with_no_export_targets() {
        // No targets means the run would produce nothing; it is rejected up front
        // rather than silently succeeding with zero documents.
        let reporter = ProgressReporter::new();
        let result = run_flow(two_region_decoded(), &config(vec![]), &reporter);
        assert!(matches!(result, Err(PipelineError::NoExportTargets)));
    }

    #[test]
    fn decoded_new_rejects_an_asset_duration_that_disagrees_with_the_samples() {
        // A probed asset longer than the decoded samples would reject the tail
        // keep region at the assemble stage; `Decoded::new` rejects it up front so
        // an inconsistent state is never constructed.
        let samples = tone_over_frames(15, &[(3, 6)]);
        let too_long = audio_asset(samples.len() + FRAME_SAMPLES);
        assert!(matches!(
            Decoded::new(too_long, samples, rate()),
            Err(FlowError::DurationMismatch { .. })
        ));
    }

    #[test]
    fn conform_snaps_regions_out_to_whole_frames_and_merges_overlaps() {
        let rate = FrameRate::whole(FPS).unwrap();
        let source = Seconds::from_frames(10, rate);
        // Two off-frame regions: [0.5, 2.5) frames and [3.5, 5.5) frames. Snapping
        // out gives [0, 3) and [3, 6), which touch and merge into [0, 6).
        let regions = vec![
            TimeRange::new(Seconds::new(1, 60).unwrap(), Seconds::new(1, 15).unwrap()).unwrap(),
            TimeRange::new(Seconds::new(7, 60).unwrap(), Seconds::new(1, 15).unwrap()).unwrap(),
        ];

        let conformed = conform_to_frames(&regions, rate, source).unwrap();

        assert_eq!(conformed.len(), 1);
        assert_eq!(conformed[0].start(), Seconds::ZERO);
        assert_eq!(conformed[0].end(), Seconds::from_frames(6, rate));
    }

    #[test]
    fn a_non_frame_aligned_window_exports_after_conforming() {
        // A 1/50 s analysis window does not divide the 30 fps grid, so the detected
        // region lands off-frame (tone over samples 3840..7680). Without
        // conforming, export would fail with UnalignedDuration; conforming snaps
        // the clip to whole frames so the frame-based exporter accepts it.
        let samples = tone_over_samples(15 * FRAME_SAMPLES, &[(3_840, 7_680)]);
        let decoded = Decoded::new(audio_asset(samples.len()), samples, rate()).unwrap();
        let off_grid = FlowConfig {
            name: "rough cut".to_owned(),
            gate: SilenceGate::new(
                Seconds::new(1, 50).unwrap(),
                Dbfs::new(-40.0),
                Seconds::ZERO,
            )
            .unwrap(),
            frame_rate: FrameRate::whole(FPS).unwrap(),
            targets: vec![ExportTarget::Xmeml],
        };
        let reporter = ProgressReporter::new();

        let exported = run_flow(decoded, &off_grid, &reporter).unwrap();

        // The off-grid region conforms to a single whole-frame clip — one clipitem,
        // not zero (dropped) or two (split) — and exporting at all proves the
        // snapped boundaries are frame-aligned, since the exporter rejects an
        // off-frame duration.
        let (_, xmeml) = exported.documents().first().unwrap();
        assert_eq!(xmeml.matches("<clipitem").count(), 1);
    }
}
