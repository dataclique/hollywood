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
//! gaps. Only if windows were correlated but none produced a peak does the map
//! surface [`SyncError::NoPeak`].
//!
//! A caller passes a coarse `base` offset — from a single [`align`](crate::align)
//! over the whole take — and each window is correlated against the target span
//! *shifted* by that base, so it measures only the small residual drift; the
//! point's offset is `base + residual`, the true lag at that window. Because each
//! window need only span the residual rather than the whole lag, sources started
//! far apart (a large base offset) need no coarser a window than tightly-aligned
//! ones. If the base is so inconsistent with the signals' lengths that no
//! window's shifted span falls within `target`, the map surfaces
//! [`SyncError::NoWindowInBounds`].

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

/// Measure how `target` drifts against `reference` (both mono, at `rate`) around
/// a coarse `base` offset, aligning each `window` of the recording with `method`.
///
/// Each reference window is correlated against the target span shifted by `base`,
/// so it measures the residual drift there; the point's offset is `base` plus
/// that residual — the true lag at the window. Pass the `base` from a single
/// [`align`](crate::align) over the whole take. Reading the points in order shows
/// the lag evolving as the clocks drift apart. Windows with no correlatable
/// content, or whose shifted span runs off `target`, are skipped, so the map may
/// be sparser than the number of windows that fit.
///
/// # Errors
///
/// [`SyncError::EmptySignal`] if either signal is empty,
/// [`SyncError::SignalShorterThanWindow`] if `reference` holds no full window,
/// [`SyncError::NoWindowInBounds`] if no window's base-shifted span falls within
/// `target` (the `base` is inconsistent with the lengths, or `target` is shorter
/// than a window), [`SyncError::NoPeak`] if windows were correlated but none
/// peaked, [`SyncError::SignalTooLong`] if a window start or composed offset is
/// unrepresentable, and any genuine per-window error from [`align`](crate::align),
/// e.g. [`SyncError::Fft`].
pub fn drift_map(
    reference: &[f32],
    target: &[f32],
    rate: SampleRate,
    base: SyncOffset,
    window: DriftWindow,
    method: CorrelationMethod,
) -> Result<DriftMap, SyncError> {
    if reference.is_empty() || target.is_empty() {
        return Err(SyncError::EmptySignal);
    }

    let length = window.length.get();
    let hop = window.hop.get();
    let base_samples = base.samples();
    let mut points = Vec::new();
    let mut any_window = false;
    let mut any_correlated = false;
    let mut start = 0_usize;
    while let Some(reference_window) = window_at(reference, start, length) {
        any_window = true;
        // Correlate against the target span shifted by the coarse `base`, so each
        // window measures only the small residual drift, not the whole lag — the
        // window then need only exceed the drift, not the total offset. A window
        // whose shifted span runs off either end of `target` (before its start or
        // past its end), or one over dead air (NoPeak), yields no point and is
        // skipped: a drift map is a sparse sampling the consumer interpolates
        // across. Genuine errors propagate.
        if let Some(target_window) = shifted_window(target, start, base_samples, length) {
            any_correlated = true;
            match align(reference_window, target_window, rate, method) {
                Ok(residual) => {
                    let total = base_samples
                        .checked_add(residual.samples())
                        .ok_or(SyncError::SignalTooLong)?;
                    let at = Seconds::from_samples(
                        i64::try_from(start).map_err(|_| SyncError::SignalTooLong)?,
                        rate,
                    );
                    points.push(DriftPoint {
                        at,
                        offset: SyncOffset::from_samples(total, rate),
                    });
                }
                Err(SyncError::NoPeak) => {}
                Err(other) => return Err(other),
            }
        }

        let Some(next) = start.checked_add(hop) else {
            break;
        };
        start = next;
    }

    if !any_window {
        return Err(SyncError::SignalShorterThanWindow);
    }
    if !any_correlated {
        return Err(SyncError::NoWindowInBounds);
    }
    if points.is_empty() {
        return Err(SyncError::NoPeak);
    }
    Ok(DriftMap { points })
}

/// The `length`-sample slice of `signal` starting at `start`, or `None` if the
/// signal has no full window left there (or the span overflows `usize`).
fn window_at(signal: &[f32], start: usize, length: usize) -> Option<&[f32]> {
    let end = start.checked_add(length)?;
    signal.get(start..end)
}

/// The `length`-sample slice of `target` for the reference window at `start`,
/// shifted by the coarse `base` offset of `samples`. `None` if the shifted span
/// falls outside `target` — before its start (a negative position) or past its
/// end — so that window yields no point.
fn shifted_window(target: &[f32], start: usize, samples: i64, length: usize) -> Option<&[f32]> {
    let shifted = i64::try_from(start).ok()?.checked_add(samples)?;
    let target_start = usize::try_from(shifted).ok()?;
    window_at(target, target_start, length)
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

    /// The coarse base offset a caller would pass, from one global [`align`].
    fn base(samples: i64) -> SyncOffset {
        SyncOffset::from_samples(samples, rate())
    }

    fn offsets(map: &DriftMap) -> Vec<i64> {
        map.points().iter().map(|p| p.offset().samples()).collect()
    }

    #[test]
    fn constant_offset_is_measured_in_every_window() {
        // Target a flat 100 samples later. With the base set to that offset, every
        // residual is zero and so every total is 100 — no drift. The target runs
        // `base` longer than the reference because it lags by that much: the
        // reference's last window needs target samples `base` past its own end.
        let reference = with_features(8_000, &[1_000, 3_000, 5_000, 7_000]);
        let target = with_features(8_100, &[1_100, 3_100, 5_100, 7_100]);

        let map = drift_map(
            &reference,
            &target,
            rate(),
            base(100),
            window(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(offsets(&map), vec![100, 100, 100, 100]);
    }

    #[test]
    fn growing_lag_shows_up_as_drift() {
        // Base 50, target falling further behind: residuals 0/10/20/30 give totals
        // that grow 50 -> 60 -> 70 -> 80, recovering the drift.
        let reference = with_features(8_000, &[1_000, 3_000, 5_000, 7_000]);
        let target = with_features(8_100, &[1_050, 3_060, 5_070, 7_080]);

        let map = drift_map(
            &reference,
            &target,
            rate(),
            base(50),
            window(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(offsets(&map), vec![50, 60, 70, 80]);
    }

    #[test]
    fn shrinking_lag_shows_up_as_drift() {
        // The mirror case: the target's clock runs fast, so the lag shrinks
        // 80 -> 70 -> 60 -> 50 — residuals around base 80 are 0/-10/-20/-30.
        let reference = with_features(8_000, &[1_000, 3_000, 5_000, 7_000]);
        let target = with_features(8_100, &[1_080, 3_070, 5_060, 7_050]);

        let map = drift_map(
            &reference,
            &target,
            rate(),
            base(80),
            window(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(offsets(&map), vec![80, 70, 60, 50]);
    }

    #[test]
    fn a_short_window_handles_a_large_base_offset() {
        // The point of measuring residual around a base: a 5000-sample base — far
        // larger than the 2000-sample window — still resolves, because each window
        // aligns against the target span shifted by the base. The old same-span
        // approach would hold non-overlapping content and fail. Totals are
        // 5000 plus the 0/10/20/30 drift.
        let reference = with_features(8_000, &[1_000, 3_000, 5_000, 7_000]);
        let target = with_features(13_000, &[6_000, 8_010, 10_020, 12_030]);

        let map = drift_map(
            &reference,
            &target,
            rate(),
            base(5_000),
            window(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(offsets(&map), vec![5_000, 5_010, 5_020, 5_030]);
    }

    #[test]
    fn a_negative_base_offset_is_measured() {
        // The target leads the reference by 4000 samples (base -4000). The early
        // windows' shifted spans fall before the start of `target` and are
        // skipped; the window that lands on the feature measures a -4000 total.
        let reference = with_features(8_000, &[5_000]);
        let target = with_features(8_000, &[1_000]);

        let map = drift_map(
            &reference,
            &target,
            rate(),
            base(-4_000),
            window(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(offsets(&map), vec![-4_000]);
    }

    #[test]
    fn points_carry_ascending_window_start_times() {
        let reference = with_features(6_000, &[1_000, 3_000, 5_000]);
        let target = with_features(6_100, &[1_100, 3_100, 5_100]);

        let map = drift_map(
            &reference,
            &target,
            rate(),
            base(100),
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
        let target = with_features(4_100, &[520, 2_520]);
        let overlapping = DriftWindow::new(2_000, 1_000).unwrap();

        let map = drift_map(
            &reference,
            &target,
            rate(),
            base(20),
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
                base(20),
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
                base(0),
                window(),
                CorrelationMethod::CrossCorrelation
            ),
            Err(SyncError::EmptySignal)
        ));
    }

    #[test]
    fn a_silent_window_is_skipped_not_fatal() {
        // The first window [0, 2000) is silent; the window at 2000 holds a feature.
        // The silent window yields no point but does not abort the map — the
        // correlated window is still measured.
        let reference = with_features(6_000, &[2_500]);
        let target = with_features(6_000, &[2_550]);
        let map = drift_map(
            &reference,
            &target,
            rate(),
            base(50),
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
                base(0),
                window(),
                CorrelationMethod::CrossCorrelation
            ),
            Err(SyncError::NoPeak)
        ));
    }

    #[test]
    fn a_base_beyond_the_target_has_no_window_in_bounds() {
        // Both signals carry correlatable content, but the base is so much larger
        // than `target` that every window's shifted span runs off its end — align
        // is never called. That is a geometry failure (a base inconsistent with
        // the lengths), surfaced as NoWindowInBounds, not NoPeak (which would
        // falsely claim the audio doesn't correlate).
        let reference = with_features(8_000, &[1_000, 3_000, 5_000, 7_000]);
        let target = with_features(5_000, &[1_100]);
        assert!(matches!(
            drift_map(
                &reference,
                &target,
                rate(),
                base(100_000),
                window(),
                CorrelationMethod::CrossCorrelation
            ),
            Err(SyncError::NoWindowInBounds)
        ));
    }

    #[test]
    fn a_target_shorter_than_the_window_has_no_window_in_bounds() {
        // The reference holds full windows but the target is shorter than one, so
        // no window's span (even at base 0) fits inside it. Distinct from a short
        // *reference* (SignalShorterThanWindow): here the reference is fine.
        let reference = with_features(4_000, &[500, 2_500]);
        let target = with_features(1_000, &[120]);
        assert!(matches!(
            drift_map(
                &reference,
                &target,
                rate(),
                base(0),
                window(),
                CorrelationMethod::CrossCorrelation
            ),
            Err(SyncError::NoWindowInBounds)
        ));
    }
}
