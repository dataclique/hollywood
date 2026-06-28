//! RMS silence gating into padded keep regions.
//!
//! The signal is split into fixed analysis windows; each window's root-mean-
//! square level is compared to a threshold to label it active (speech) or silent
//! (dead air). Runs of active windows become regions, each padded so speech
//! onsets and tails are not clipped. Padding also bridges short pauses: two
//! regions merge when the gap between them is at most twice the padding. A pause
//! must span at least one full analysis window to be detected as a gap at all,
//! so sub-window pauses never split a run.

use hollywood_timeline::{SampleRate, Seconds, TimeRange};

use crate::error::DetectError;

/// A level in decibels relative to full scale (`0 dBFS` = amplitude `1.0`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dbfs(f32);

impl Dbfs {
    /// A dBFS level. Silence thresholds are negative (e.g. `-40.0`).
    pub fn new(decibels: f32) -> Self {
        Self(decibels)
    }

    /// The equivalent linear amplitude, `10^(dBFS / 20)`.
    pub fn linear(self) -> f64 {
        10f64.powf(f64::from(self.0) / 20.0)
    }
}

/// Configuration for [`keep_regions`].
#[derive(Clone, Copy, Debug)]
pub struct SilenceGate {
    window: Seconds,
    threshold: Dbfs,
    padding: Seconds,
}

impl SilenceGate {
    /// A gate that labels a window silent when its RMS falls below `threshold`,
    /// analyzing in `window`-long steps and padding each kept region by
    /// `padding` on both sides.
    ///
    /// # Errors
    ///
    /// [`DetectError::NonPositiveWindow`] if `window` is not positive,
    /// [`DetectError::NegativePadding`] if `padding` is negative, and
    /// [`DetectError::InvalidThreshold`] if `threshold` does not convert to a
    /// finite, positive amplitude (which would invert or disable gating).
    pub fn new(window: Seconds, threshold: Dbfs, padding: Seconds) -> Result<Self, DetectError> {
        if window.is_negative() || window.is_zero() {
            return Err(DetectError::NonPositiveWindow);
        }
        if padding.is_negative() {
            return Err(DetectError::NegativePadding);
        }
        let linear = threshold.linear();
        if !linear.is_finite() || linear <= 0.0 {
            return Err(DetectError::InvalidThreshold);
        }
        Ok(Self {
            window,
            threshold,
            padding,
        })
    }

    /// The analysis window duration.
    pub fn window(self) -> Seconds {
        self.window
    }

    /// The silence threshold.
    pub fn threshold(self) -> Dbfs {
        self.threshold
    }

    /// The padding applied to each side of a kept region.
    pub fn padding(self) -> Seconds {
        self.padding
    }
}

/// The keep regions of `samples` (mono, in `[-1.0, 1.0]`) at `rate` under `gate`,
/// in ascending order with no overlaps. Silent spans between regions are the cut
/// (dead air); empty input yields no regions.
///
/// # Errors
///
/// [`DetectError::InvalidWindow`] if the window spans no usable samples at
/// `rate`, [`DetectError::NonFiniteSample`] if the signal contains a NaN or
/// infinity, [`DetectError::DurationOverflow`] if a region exceeds representable
/// time, and [`DetectError::Timeline`] if the IR rejects a region.
pub fn keep_regions(
    samples: &[f32],
    rate: SampleRate,
    gate: &SilenceGate,
) -> Result<Vec<TimeRange>, DetectError> {
    let window_samples = gate
        .window
        .checked_to_samples(rate)
        .and_then(|count| usize::try_from(count).ok())
        .filter(|&count| count >= 1)
        .ok_or(DetectError::InvalidWindow)?;
    let threshold = gate.threshold.linear();

    let runs = active_runs(samples, window_samples, threshold)?;
    let total = samples_to_seconds(samples.len(), rate)?;

    let mut keep: Vec<Span> = Vec::new();
    for run in runs {
        let start = pad_start(samples_to_seconds(run.start, rate)?, gate.padding)?;
        let end = pad_end(samples_to_seconds(run.end, rate)?, gate.padding, total)?;
        match keep.last_mut() {
            // Overlapping or padding-adjacent: extend the previous region.
            Some(last) if start <= last.end => last.end = last.end.max(end),
            _ => keep.push(Span { start, end }),
        }
    }

    keep.into_iter()
        .map(|span| {
            let duration = span
                .end
                .checked_sub(span.start)
                .ok_or(DetectError::DurationOverflow)?;
            Ok(TimeRange::new(span.start, duration)?)
        })
        .collect()
}

/// A keep region as a half-open `[start, end)` span in seconds.
struct Span {
    start: Seconds,
    end: Seconds,
}

/// A half-open `[start, end)` span of samples covering consecutive active windows.
struct SampleSpan {
    start: usize,
    end: usize,
}

/// The sample spans of consecutive active windows, in order.
fn active_runs(
    samples: &[f32],
    window_samples: usize,
    threshold: f64,
) -> Result<Vec<SampleSpan>, DetectError> {
    let mut runs = Vec::new();
    let mut offset = 0usize;
    let mut current: Option<usize> = None;
    for window in samples.chunks(window_samples) {
        let active = window_rms(window)? >= threshold;
        let start = offset;
        // Advance by the window's own length so a short final chunk is exact.
        offset = offset.saturating_add(window.len());
        if active {
            current.get_or_insert(start);
        } else if let Some(run_start) = current.take() {
            runs.push(SampleSpan {
                start: run_start,
                end: start,
            });
        }
    }
    if let Some(run_start) = current.take() {
        runs.push(SampleSpan {
            start: run_start,
            end: offset,
        });
    }
    Ok(runs)
}

/// Root-mean-square level of one window. An empty window has none (`0.0`); a
/// window containing a non-finite sample is an error rather than a silent
/// misclassification.
fn window_rms(window: &[f32]) -> Result<f64, DetectError> {
    // `u32 -> f64` is lossless; a window never holds more than `u32::MAX` samples.
    let count = u32::try_from(window.len()).map_err(|_| DetectError::InvalidWindow)?;
    if count == 0 {
        return Ok(0.0);
    }
    let sum_squares: f64 = window
        .iter()
        .map(|&sample| f64::from(sample) * f64::from(sample))
        .sum();
    let rms = (sum_squares / f64::from(count)).sqrt();
    if rms.is_finite() {
        Ok(rms)
    } else {
        Err(DetectError::NonFiniteSample)
    }
}

fn samples_to_seconds(count: usize, rate: SampleRate) -> Result<Seconds, DetectError> {
    let count = i64::try_from(count).map_err(|_| DetectError::DurationOverflow)?;
    Ok(Seconds::from_samples(count, rate))
}

/// `start - padding`, clamped to the timeline origin.
fn pad_start(start: Seconds, padding: Seconds) -> Result<Seconds, DetectError> {
    let padded = start
        .checked_sub(padding)
        .ok_or(DetectError::DurationOverflow)?;
    Ok(if padded.is_negative() {
        Seconds::ZERO
    } else {
        padded
    })
}

/// `end + padding`, clamped to the signal's total length.
fn pad_end(end: Seconds, padding: Seconds, total: Seconds) -> Result<Seconds, DetectError> {
    let padded = end
        .checked_add(padding)
        .ok_or(DetectError::DurationOverflow)?;
    Ok(padded.min(total))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE_HZ: u32 = 8_000;

    fn rate() -> SampleRate {
        SampleRate::new(RATE_HZ).unwrap()
    }

    /// `window` of 50 ms, `-20 dBFS` threshold (linear 0.1), `padding` 100 ms.
    fn gate() -> SilenceGate {
        SilenceGate::new(
            Seconds::new(1, 20).unwrap(),
            Dbfs::new(-20.0),
            Seconds::new(1, 10).unwrap(),
        )
        .unwrap()
    }

    /// `seconds` of constant amplitude `level` at `RATE_HZ`.
    fn block(level: f32, seconds: i64) -> Vec<f32> {
        vec![level; usize::try_from(seconds * i64::from(RATE_HZ)).unwrap()]
    }

    /// `count` samples of silence (sub-second gaps the `block` helper can't make).
    fn silence(count: usize) -> Vec<f32> {
        vec![0.0; count]
    }

    fn seconds(numerator: i64, denominator: i64) -> Seconds {
        Seconds::new(numerator, denominator).unwrap()
    }

    #[test]
    fn silence_then_tone_then_silence_is_one_padded_region() {
        // 1 s silence, 1 s at 0.5 (RMS 0.5 > 0.1), 1 s silence.
        let mut samples = block(0.0, 1);
        samples.extend(block(0.5, 1));
        samples.extend(block(0.0, 1));

        let regions = keep_regions(&samples, rate(), &gate()).unwrap();

        // Active run [1 s, 2 s] padded by 100 ms -> [0.9 s, 2.1 s].
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start(), seconds(9, 10));
        assert_eq!(regions[0].end(), seconds(21, 10));
    }

    #[test]
    fn all_silence_is_no_regions() {
        let samples = block(0.0, 3);
        assert!(keep_regions(&samples, rate(), &gate()).unwrap().is_empty());
    }

    #[test]
    fn empty_input_is_no_regions() {
        assert!(keep_regions(&[], rate(), &gate()).unwrap().is_empty());
    }

    #[test]
    fn all_active_is_one_region_clamped_to_the_signal() {
        let samples = block(0.5, 2);
        let regions = keep_regions(&samples, rate(), &gate()).unwrap();
        // Padding cannot extend past the signal: [0 s, 2 s].
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start(), Seconds::ZERO);
        assert_eq!(regions[0].end(), Seconds::from_secs(2));
    }

    #[test]
    fn alternating_signal_is_detected_active() {
        // RMS of +/-0.5 is 0.5 (> 0.1); a sum-instead-of-squares bug would read
        // ~0 and miss it. Whole signal active -> one region [0 s, 1 s].
        let samples: Vec<f32> = (0..usize::try_from(RATE_HZ).unwrap())
            .map(|index| if index % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let regions = keep_regions(&samples, rate(), &gate()).unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start(), Seconds::ZERO);
        assert_eq!(regions[0].end(), Seconds::from_secs(1));
    }

    #[test]
    fn short_gap_within_padding_is_bridged() {
        // tone, 0.1 s silence (< 2x padding = 0.2 s), tone -> one merged region.
        let mut samples = block(0.5, 1);
        samples.extend(silence(usize::try_from(RATE_HZ / 10).unwrap()));
        samples.extend(block(0.5, 1));

        let regions = keep_regions(&samples, rate(), &gate()).unwrap();
        // Both ends clamp: [0 s, 2.1 s] (total = 2.1 s).
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start(), Seconds::ZERO);
        assert_eq!(regions[0].end(), seconds(21, 10));
    }

    #[test]
    fn gap_equal_to_twice_padding_is_bridged() {
        // 0.2 s gap == 2x padding: padded boundaries touch exactly -> merge.
        let mut samples = block(0.5, 1);
        samples.extend(silence(usize::try_from(RATE_HZ / 5).unwrap())); // 0.2 s
        samples.extend(block(0.5, 1));

        let regions = keep_regions(&samples, rate(), &gate()).unwrap();
        // total = 2.2 s; merged and clamped -> [0 s, 2.2 s].
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start(), Seconds::ZERO);
        assert_eq!(regions[0].end(), seconds(11, 5));
    }

    #[test]
    fn long_gap_beyond_padding_stays_cut() {
        // 1 s silence (> 2x padding) -> two regions with exact boundaries.
        let mut samples = block(0.5, 1);
        samples.extend(block(0.0, 1));
        samples.extend(block(0.5, 1));

        let regions = keep_regions(&samples, rate(), &gate()).unwrap();
        assert_eq!(regions.len(), 2);
        // [0 s, 1.1 s] and [1.9 s, 3 s].
        assert_eq!(regions[0].start(), Seconds::ZERO);
        assert_eq!(regions[0].end(), seconds(11, 10));
        assert_eq!(regions[1].start(), seconds(19, 10));
        assert_eq!(regions[1].end(), Seconds::from_secs(3));
    }

    #[test]
    fn partial_final_window_is_captured() {
        // 1 s tone (whole windows) + 200 active samples (a partial final chunk).
        let mut samples = block(0.5, 1);
        samples.extend(vec![0.5_f32; 200]);

        let regions = keep_regions(&samples, rate(), &gate()).unwrap();
        // Run reaches the true end; padded end clamps to total = 8200/8000 s.
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start(), Seconds::ZERO);
        assert_eq!(regions[0].end(), seconds(41, 40));
    }

    #[test]
    fn window_below_one_sample_is_invalid() {
        // 1/100000 s at 8000 Hz is 0.08 samples -> rounds to 0.
        let tiny = SilenceGate::new(seconds(1, 100_000), Dbfs::new(-20.0), seconds(1, 10)).unwrap();
        let samples = block(0.5, 1);
        assert!(matches!(
            keep_regions(&samples, rate(), &tiny),
            Err(DetectError::InvalidWindow)
        ));
    }

    #[test]
    fn non_finite_samples_are_rejected() {
        let mut samples = block(0.5, 1);
        samples[0] = f32::NAN;
        assert!(matches!(
            keep_regions(&samples, rate(), &gate()),
            Err(DetectError::NonFiniteSample)
        ));
    }

    #[test]
    fn gate_rejects_degenerate_threshold() {
        let window = seconds(1, 20);
        let padding = Seconds::ZERO;
        // -inf dBFS underflows to linear 0 (would label silence active).
        assert!(matches!(
            SilenceGate::new(window, Dbfs::new(f32::NEG_INFINITY), padding),
            Err(DetectError::InvalidThreshold)
        ));
        // A huge positive dBFS overflows to infinite linear.
        assert!(matches!(
            SilenceGate::new(window, Dbfs::new(10_000.0), padding),
            Err(DetectError::InvalidThreshold)
        ));
    }

    #[test]
    fn gate_rejects_non_positive_window() {
        assert!(matches!(
            SilenceGate::new(Seconds::ZERO, Dbfs::new(-40.0), Seconds::ZERO),
            Err(DetectError::NonPositiveWindow)
        ));
    }

    #[test]
    fn gate_rejects_negative_padding() {
        assert!(matches!(
            SilenceGate::new(seconds(1, 20), Dbfs::new(-40.0), seconds(-1, 10)),
            Err(DetectError::NegativePadding)
        ));
    }

    #[test]
    fn gate_exposes_its_configuration() {
        let configured = gate();
        assert_eq!(configured.window(), seconds(1, 20));
        assert_eq!(configured.threshold(), Dbfs::new(-20.0));
        assert_eq!(configured.padding(), seconds(1, 10));
    }

    #[test]
    fn dbfs_converts_to_linear_amplitude() {
        assert!((Dbfs::new(0.0).linear() - 1.0).abs() < 1e-9);
        assert!((Dbfs::new(-20.0).linear() - 0.1).abs() < 1e-9);
    }
}
