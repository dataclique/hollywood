//! RMS silence gating into padded keep regions.
//!
//! The signal is split into fixed analysis windows; each window's root-mean-
//! square level is compared to a threshold to label it active (speech) or silent
//! (dead air). Runs of active windows become regions, each padded so speech
//! onsets and tails are not clipped — and so brief pauses shorter than the
//! padding on each side are bridged rather than cut into choppy fragments.

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
    /// [`DetectError::NonFiniteThreshold`] if `threshold` is not finite.
    pub fn new(window: Seconds, threshold: Dbfs, padding: Seconds) -> Result<Self, DetectError> {
        if window.is_negative() || window.is_zero() {
            return Err(DetectError::NonPositiveWindow);
        }
        if padding.is_negative() {
            return Err(DetectError::NegativePadding);
        }
        if !threshold.linear().is_finite() {
            return Err(DetectError::NonFiniteThreshold);
        }
        Ok(Self {
            window,
            threshold,
            padding,
        })
    }
}

/// The keep regions of `samples` (mono, in `[-1.0, 1.0]`) at `rate` under `gate`,
/// in ascending order with no overlaps. Silent spans between regions are the cut
/// (dead air); empty input yields no regions.
///
/// # Errors
///
/// [`DetectError::InvalidWindow`] if the window spans no usable samples at
/// `rate`, [`DetectError::DurationOverflow`] if a region exceeds representable
/// time, and [`DetectError::Timeline`] if the IR rejects a region.
pub fn keep_regions(
    samples: &[f32],
    rate: SampleRate,
    gate: &SilenceGate,
) -> Result<Vec<TimeRange>, DetectError> {
    let window_samples = usize::try_from(gate.window.to_samples(rate))
        .ok()
        .filter(|&count| count >= 1)
        .ok_or(DetectError::InvalidWindow)?;
    let threshold = gate.threshold.linear();

    let active_runs = active_runs(samples, window_samples, threshold)?;
    let total = samples_to_seconds(samples.len(), rate)?;

    let mut keep: Vec<(Seconds, Seconds)> = Vec::new();
    for (start, end) in active_runs {
        let region_start = pad_start(samples_to_seconds(start, rate)?, gate.padding)?;
        let region_end = pad_end(samples_to_seconds(end, rate)?, gate.padding, total)?;
        match keep.last_mut() {
            // Overlapping or padding-adjacent: extend the previous region.
            Some(last) if region_start <= last.1 => last.1 = last.1.max(region_end),
            _ => keep.push((region_start, region_end)),
        }
    }

    keep.into_iter()
        .map(|(start, end)| {
            let duration = end
                .checked_sub(start)
                .ok_or(DetectError::DurationOverflow)?;
            Ok(TimeRange::new(start, duration)?)
        })
        .collect()
}

/// The half-open sample spans `[start, end)` of consecutive active windows.
fn active_runs(
    samples: &[f32],
    window_samples: usize,
    threshold: f64,
) -> Result<Vec<(usize, usize)>, DetectError> {
    let mut runs = Vec::new();
    let mut offset = 0usize;
    let mut current: Option<usize> = None;
    for window in samples.chunks(window_samples) {
        let active = window_rms(window)? >= threshold;
        let start = offset;
        offset = offset.saturating_add(window.len());
        if active {
            current.get_or_insert(start);
        } else if let Some(run_start) = current.take() {
            runs.push((run_start, start));
        }
    }
    if let Some(run_start) = current.take() {
        runs.push((run_start, offset));
    }
    Ok(runs)
}

/// Root-mean-square level of one window. An empty window has none (`0.0`).
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
    Ok((sum_squares / f64::from(count)).sqrt())
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

    #[test]
    fn silence_then_tone_then_silence_is_one_padded_region() {
        // 1 s silence, 1 s at 0.5 (RMS 0.5 > 0.1), 1 s silence.
        let mut samples = block(0.0, 1);
        samples.extend(block(0.5, 1));
        samples.extend(block(0.0, 1));

        let regions = keep_regions(&samples, rate(), &gate()).unwrap();

        // Active run [1 s, 2 s] padded by 100 ms -> [0.9 s, 2.1 s].
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start(), Seconds::new(9, 10).unwrap());
        assert_eq!(regions[0].end(), Seconds::new(21, 10).unwrap());
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
    fn short_gap_within_padding_is_bridged() {
        // tone, 0.1 s silence (< 2x padding), tone -> one merged region.
        let mut samples = block(0.5, 1);
        samples.extend(vec![0.0; usize::try_from(RATE_HZ / 10).unwrap()]);
        samples.extend(block(0.5, 1));

        let regions = keep_regions(&samples, rate(), &gate()).unwrap();
        assert_eq!(regions.len(), 1);
    }

    #[test]
    fn long_gap_beyond_padding_stays_cut() {
        // tone, 1 s silence (> 2x padding), tone -> two regions.
        let mut samples = block(0.5, 1);
        samples.extend(block(0.0, 1));
        samples.extend(block(0.5, 1));

        let regions = keep_regions(&samples, rate(), &gate()).unwrap();
        assert_eq!(regions.len(), 2);
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
        let negative = Seconds::new(-1, 10).unwrap();
        assert!(matches!(
            SilenceGate::new(Seconds::new(1, 20).unwrap(), Dbfs::new(-40.0), negative),
            Err(DetectError::NegativePadding)
        ));
    }

    #[test]
    fn dbfs_converts_to_linear_amplitude() {
        assert!((Dbfs::new(0.0).linear() - 1.0).abs() < 1e-9);
        assert!((Dbfs::new(-20.0).linear() - 0.1).abs() < 1e-9);
    }
}
