//! Piecewise drift map for long recordings.
//!
//! A single [`align`](crate::align) reports one offset between two sources, which
//! is enough when their clocks tick at the same rate. Over a long take they
//! rarely do: a few parts-per-million of clock drift accumulates into a lag that
//! grows across the recording, so one offset is right only near the moment it was
//! measured. [`drift_map`] instead slides a window across both signals and aligns
//! each window in turn, producing a [`DriftMap`] — the offset sampled over time,
//! from which the assembler can correct drift rather than assume a fixed lag.
//!
//! Each window is aligned independently. A window with no correlatable content
//! (dead air) yields no point — it is skipped, so the map is a sparse sampling
//! over the windows that did correlate and the consumer interpolates across the
//! gaps. Only if no window correlates at all does the map surface
//! [`SyncError::NoPeak`].
//!
//! Both windows cover the *same* sample span, so each measures the **total** lag
//! at that point (base offset plus accumulated drift), not the drift increment,
//! and the window must be longer than that total lag — otherwise the two spans
//! hold non-overlapping content. For sources started far apart (a large base
//! offset) that forces a coarse window; correlating against a target span shifted
//! by a coarse global offset, to measure only the small residual drift, is the
//! robust refinement and a follow-up.

use std::num::NonZeroUsize;

use hollywood_timeline::{SampleRate, Seconds};

use crate::alignment::{CorrelationMethod, SyncOffset, align};
use crate::error::SyncError;

/// The offset between two sources sampled over a recording, one [`DriftPoint`]
/// per window in ascending time order.
///
/// A constant offset means the clocks agree; an offset that grows or shrinks
/// across the points is clock drift.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriftMap {
    points: Vec<DriftPoint>,
}

impl DriftMap {
    /// The measured points, one per window, in ascending time order.
    pub fn points(&self) -> &[DriftPoint] {
        &self.points
    }
}

/// The alignment measured in one window: the offset, and the window's start time
/// in the reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriftPoint {
    at: Seconds,
    offset: SyncOffset,
}

impl DriftPoint {
    /// When the window starts, in the reference's timebase.
    pub fn at(self) -> Seconds {
        self.at
    }

    /// How far `target` lags `reference` over this window.
    pub fn offset(self) -> SyncOffset {
        self.offset
    }
}

/// How [`drift_map`] divides a recording into windows.
///
/// Each window spans `length` samples and the next starts `hop` samples later.
/// Both are non-zero, so an empty or non-advancing window cannot be constructed.
#[derive(Clone, Copy, Debug)]
pub struct DriftWindow {
    length: NonZeroUsize,
    hop: NonZeroUsize,
}

impl DriftWindow {
    /// A window of `length` samples advancing by `hop` samples. Returns `None` if
    /// either is zero — an empty window correlates nothing, and a zero hop never
    /// advances. A `hop` below `length` overlaps successive windows; `hop` equal
    /// to `length` tiles them edge to edge.
    pub fn new(length: usize, hop: usize) -> Option<Self> {
        Some(Self {
            length: NonZeroUsize::new(length)?,
            hop: NonZeroUsize::new(hop)?,
        })
    }
}

/// Measure how `target` drifts against `reference` (both mono, at `rate`) by
/// aligning each `window` of the recording with `method`.
///
/// Each window of `reference` is aligned against the same span of `target`, so
/// each point is the total lag at that window (base offset plus drift); reading
/// the points in order shows the lag evolving as the clocks drift apart. Windows
/// with no correlatable content are skipped, so the map may be sparser than the
/// number of windows that fit.
///
/// # Errors
///
/// [`SyncError::EmptySignal`] if either signal is empty,
/// [`SyncError::SignalShorterThanWindow`] if neither holds one full window,
/// [`SyncError::NoPeak`] if windows fit but none correlated, and any genuine
/// per-window error from [`align`](crate::align), e.g. [`SyncError::Fft`].
pub fn drift_map(
    reference: &[f32],
    target: &[f32],
    rate: SampleRate,
    window: DriftWindow,
    method: CorrelationMethod,
) -> Result<DriftMap, SyncError> {
    if reference.is_empty() || target.is_empty() {
        return Err(SyncError::EmptySignal);
    }

    let length = window.length.get();
    let hop = window.hop.get();
    let mut points = Vec::new();
    let mut any_window = false;
    let mut start = 0_usize;
    while let Some((reference_window, target_window)) = windows_at(reference, target, start, length)
    {
        any_window = true;
        // A window over dead air (or otherwise uncorrelatable content) yields no
        // measurement: skip it and carry on, rather than abort the whole map. A
        // drift map is a sparse sampling and raw footage is full of pauses, so a
        // silent window is the norm, not a failure — the consumer interpolates
        // across the gaps. Genuine errors (FFT, overflow) still propagate.
        match align(reference_window, target_window, rate, method) {
            Ok(offset) => {
                let at = Seconds::from_samples(
                    i64::try_from(start).map_err(|_| SyncError::SignalTooLong)?,
                    rate,
                );
                points.push(DriftPoint { at, offset });
            }
            Err(SyncError::NoPeak) => {}
            Err(other) => return Err(other),
        }

        let Some(next) = start.checked_add(hop) else {
            break;
        };
        start = next;
    }

    if !any_window {
        return Err(SyncError::SignalShorterThanWindow);
    }
    if points.is_empty() {
        return Err(SyncError::NoPeak);
    }
    Ok(DriftMap { points })
}

/// The `length`-sample slice of each signal starting at `start`, or `None` once
/// either signal has no full window left (or the span overflows `usize`).
fn windows_at<'a>(
    reference: &'a [f32],
    target: &'a [f32],
    start: usize,
    length: usize,
) -> Option<(&'a [f32], &'a [f32])> {
    let end = start.checked_add(length)?;
    Some((reference.get(start..end)?, target.get(start..end)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE_HZ: u32 = 48_000;

    fn rate() -> SampleRate {
        SampleRate::new(RATE_HZ).unwrap()
    }

    const PATTERN: [f32; 4] = [0.2, 0.9, -0.5, 0.3];

    /// A `len`-sample signal with `PATTERN` written at each of `positions`.
    fn with_features(len: usize, positions: &[usize]) -> Vec<f32> {
        let mut signal = vec![0.0; len];
        for &position in positions {
            for (offset, &value) in PATTERN.iter().enumerate() {
                if let Some(slot) = signal.get_mut(position + offset) {
                    *slot = value;
                }
            }
        }
        signal
    }

    fn window() -> DriftWindow {
        DriftWindow::new(2_000, 2_000).unwrap()
    }

    fn offsets(map: &DriftMap) -> Vec<i64> {
        map.points().iter().map(|p| p.offset().samples()).collect()
    }

    #[test]
    fn constant_offset_is_measured_in_every_window() {
        // Features every 2000 samples, target a flat 100 samples later: no drift.
        let reference = with_features(8_000, &[1_000, 3_000, 5_000, 7_000]);
        let target = with_features(8_000, &[1_100, 3_100, 5_100, 7_100]);

        let map = drift_map(
            &reference,
            &target,
            rate(),
            window(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(offsets(&map), vec![100, 100, 100, 100]);
    }

    #[test]
    fn growing_lag_shows_up_as_drift() {
        // The target's feature falls progressively further behind: the per-window
        // offset grows 50 -> 60 -> 70 -> 80, recovering the drift.
        let reference = with_features(8_000, &[1_000, 3_000, 5_000, 7_000]);
        let target = with_features(8_000, &[1_050, 3_060, 5_070, 7_080]);

        let map = drift_map(
            &reference,
            &target,
            rate(),
            window(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(offsets(&map), vec![50, 60, 70, 80]);
    }

    #[test]
    fn shrinking_lag_shows_up_as_drift() {
        // The mirror case: the target's clock runs fast, so the lag shrinks
        // 80 -> 70 -> 60 -> 50 across the recording — drift is directional.
        let reference = with_features(8_000, &[1_000, 3_000, 5_000, 7_000]);
        let target = with_features(8_000, &[1_080, 3_070, 5_060, 7_050]);

        let map = drift_map(
            &reference,
            &target,
            rate(),
            window(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(offsets(&map), vec![80, 70, 60, 50]);
    }

    #[test]
    fn points_carry_ascending_window_start_times() {
        let reference = with_features(6_000, &[1_000, 3_000, 5_000]);
        let target = with_features(6_000, &[1_100, 3_100, 5_100]);

        let map = drift_map(
            &reference,
            &target,
            rate(),
            window(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        let times: Vec<Seconds> = map.points().iter().map(|p| p.at()).collect();
        assert_eq!(
            times,
            vec![
                Seconds::ZERO,
                Seconds::from_samples(2_000, rate()),
                Seconds::from_samples(4_000, rate()),
            ]
        );
    }

    #[test]
    fn overlapping_windows_advance_by_the_hop() {
        // Window 2000, hop 1000 over 4000 samples yields windows at 0, 1000, 2000.
        let reference = with_features(4_000, &[500, 2_500]);
        let target = with_features(4_000, &[520, 2_520]);
        let overlapping = DriftWindow::new(2_000, 1_000).unwrap();

        let map = drift_map(
            &reference,
            &target,
            rate(),
            overlapping,
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        // Three overlapping windows advanced by the hop: [0,2000), [1000,3000),
        // [2000,4000) — each independently resolves the +20 offset.
        let starts: Vec<Seconds> = map.points().iter().map(|p| p.at()).collect();
        assert_eq!(
            starts,
            vec![
                Seconds::ZERO,
                Seconds::from_samples(1_000, rate()),
                Seconds::from_samples(2_000, rate()),
            ]
        );
        assert_eq!(offsets(&map), vec![20, 20, 20]);
    }

    #[test]
    fn degenerate_windows_are_unrepresentable() {
        assert!(DriftWindow::new(0, 2_000).is_none());
        assert!(DriftWindow::new(2_000, 0).is_none());
    }

    #[test]
    fn signal_shorter_than_the_window_is_an_error() {
        let reference = with_features(1_000, &[100]);
        let target = with_features(1_000, &[120]);
        assert!(matches!(
            drift_map(
                &reference,
                &target,
                rate(),
                window(),
                CorrelationMethod::CrossCorrelation
            ),
            Err(SyncError::SignalShorterThanWindow)
        ));
    }

    #[test]
    fn empty_signal_is_an_error() {
        let target = with_features(4_000, &[100]);
        assert!(matches!(
            drift_map(
                &[],
                &target,
                rate(),
                window(),
                CorrelationMethod::CrossCorrelation
            ),
            Err(SyncError::EmptySignal)
        ));
    }

    #[test]
    fn a_silent_window_is_skipped_not_fatal() {
        // The first window [0, 2000) is silent; the second [2000, 4000) holds a
        // feature. The silent window yields no point but does not abort the map —
        // the correlated window is still measured.
        let reference = with_features(4_000, &[2_500]);
        let target = with_features(4_000, &[2_550]);
        let map = drift_map(
            &reference,
            &target,
            rate(),
            window(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(map.points().len(), 1);
        assert_eq!(map.points()[0].at(), Seconds::from_samples(2_000, rate()));
        assert_eq!(offsets(&map), vec![50]);
    }

    #[test]
    fn all_silent_windows_yield_no_peak() {
        // Windows fit but none correlate, so the map has no points — surfaced as
        // NoPeak, distinct from "no window fit at all" (SignalShorterThanWindow).
        let silence = vec![0.0_f32; 4_000];
        assert!(matches!(
            drift_map(
                &silence,
                &silence,
                rate(),
                window(),
                CorrelationMethod::CrossCorrelation
            ),
            Err(SyncError::NoPeak)
        ));
    }
}
