//! FFT cross-correlation alignment.
//!
//! The cross-correlation of two signals peaks at the lag that best aligns them.
//! Computed via the convolution theorem: `IFFT(conj(R) · T)` where `R`, `T` are
//! the signals' spectra. Both signals are zero-padded to at least their combined
//! length so the circular correlation the FFT yields equals the linear one.
//!
//! [`CorrelationMethod`] selects how the cross-power spectrum is weighted before
//! the inverse transform: plain cross-correlation, or the Phase Transform
//! ([`CorrelationMethod::Phat`]) which whitens every bin to unit magnitude for a
//! sharp, amplitude-invariant peak on spectrally-colored material.
//!
//! Valid lags run from `-(reference_len - 1)` to `target_len - 1`, so the peak's
//! buffer index maps to a lag by length, not by a fixed midpoint: indices below
//! `target_len` are non-negative lags (`target` starts later); indices in the
//! tail, within `reference_len - 1` of the end, are negative lags that wrapped
//! around; the zero-padding gap between them holds no valid lag.

use hollywood_timeline::{SampleRate, Seconds};
use realfft::RealToComplex;
use realfft::num_complex::Complex;

use crate::error::SyncError;

/// How the cross-power spectrum is weighted before the inverse transform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorrelationMethod {
    /// Plain cross-correlation `conj(R)·T`. The peak's shape follows the
    /// signals' autocorrelation; simple and fast, but the finite-overlap
    /// envelope and amplitude differences can bias or blunt it.
    CrossCorrelation,
    /// Phase Transform (GCC-PHAT): whiten each bin to unit magnitude, keeping
    /// only phase. The peak becomes a sharp, amplitude-invariant impulse,
    /// robust to colored spectra — at the cost of amplifying empty bins' noise.
    Phat,
}

/// How far `target` lags `reference`: positive means `target` starts later,
/// negative means it starts earlier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncOffset {
    samples: i64,
    rate: SampleRate,
}

impl SyncOffset {
    /// The offset in samples (positive = `target` lags `reference`).
    pub fn samples(self) -> i64 {
        self.samples
    }

    /// The offset as a duration at the signals' sample rate.
    pub fn seconds(self) -> Seconds {
        Seconds::from_samples(self.samples, self.rate)
    }
}

/// Align `target` to `reference` (both mono, at `rate`) by cross-correlation
/// under `method`, returning how far `target` lags `reference`.
///
/// # Errors
///
/// [`SyncError::EmptySignal`] if either signal is empty,
/// [`SyncError::SignalTooLong`] if their combined length is unrepresentable,
/// [`SyncError::Fft`] if the transform fails, and [`SyncError::NoPeak`] if the
/// signals are silent or uncorrelated (no positive correlation peak).
pub fn align(
    reference: &[f32],
    target: &[f32],
    rate: SampleRate,
    method: CorrelationMethod,
) -> Result<SyncOffset, SyncError> {
    if reference.is_empty() || target.is_empty() {
        return Err(SyncError::EmptySignal);
    }

    let correlation_len = reference
        .len()
        .checked_add(target.len())
        .and_then(|sum| sum.checked_sub(1))
        .ok_or(SyncError::SignalTooLong)?;
    let fft_len = correlation_len
        .checked_next_power_of_two()
        .ok_or(SyncError::SignalTooLong)?;

    let mut planner = realfft::RealFftPlanner::<f32>::new();
    let forward = planner.plan_fft_forward(fft_len);
    let inverse = planner.plan_fft_inverse(fft_len);

    let reference_spectrum = spectrum(forward.as_ref(), reference)?;
    let target_spectrum = spectrum(forward.as_ref(), target)?;

    // Cross-power spectrum conj(R)·T, optionally whitened; its inverse transform
    // is the correlation.
    let mut cross = forward.make_output_vec();
    for ((slot, &r), &t) in cross
        .iter_mut()
        .zip(&reference_spectrum)
        .zip(&target_spectrum)
    {
        let cross_power = r.conj() * t;
        *slot = match method {
            CorrelationMethod::CrossCorrelation => cross_power,
            CorrelationMethod::Phat => whiten(cross_power),
        };
    }
    // The C2R transform requires the DC and Nyquist bins to be purely real; for
    // real inputs they already are, bar floating-point dust.
    if let Some(dc) = cross.first_mut() {
        dc.im = 0.0;
    }
    if let Some(nyquist) = cross.last_mut() {
        nyquist.im = 0.0;
    }

    let mut correlation = inverse.make_output_vec();
    inverse.process(&mut cross, &mut correlation)?;

    let peak = argmax(&correlation).ok_or(SyncError::NoPeak)?;
    // A non-positive maximum means there is no real correlation (silent or
    // uncorrelated signals), not a spurious sub-sample offset.
    if correlation.get(peak).copied().ok_or(SyncError::NoPeak)? <= 0.0 {
        return Err(SyncError::NoPeak);
    }
    Ok(SyncOffset {
        samples: lag_from_index(peak, fft_len, reference.len(), target.len())?,
        rate,
    })
}

/// Whiten a cross-power bin to unit magnitude (keep phase only). An empty bin
/// carries no phase, so it stays zero rather than amplifying to unit noise.
fn whiten(bin: Complex<f32>) -> Complex<f32> {
    let magnitude = bin.norm();
    if magnitude > f32::EPSILON {
        bin.unscale(magnitude)
    } else {
        Complex::default()
    }
}

/// The spectrum of `signal` zero-padded to the transform's length.
fn spectrum(
    forward: &dyn RealToComplex<f32>,
    signal: &[f32],
) -> Result<Vec<Complex<f32>>, SyncError> {
    let mut input = forward.make_input_vec();
    for (slot, &sample) in input.iter_mut().zip(signal) {
        *slot = sample;
    }
    let mut output = forward.make_output_vec();
    forward.process(&mut input, &mut output)?;
    Ok(output)
}

/// The index of the largest correlation value.
fn argmax(values: &[f32]) -> Option<usize> {
    values
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(index, _)| index)
}

/// Map a correlation-buffer index to a signed lag using the signals' lengths.
///
/// Valid lags occupy `[0, target_len)` at the buffer's head and
/// `[-(reference_len - 1), 0)` at its tail; an index in the zero-padding gap
/// between them corresponds to no real lag and yields [`SyncError::NoPeak`].
fn lag_from_index(
    index: usize,
    fft_len: usize,
    reference_len: usize,
    target_len: usize,
) -> Result<i64, SyncError> {
    let index = i64::try_from(index).map_err(|_| SyncError::SignalTooLong)?;
    let fft_len = i64::try_from(fft_len).map_err(|_| SyncError::SignalTooLong)?;
    let reference_len = i64::try_from(reference_len).map_err(|_| SyncError::SignalTooLong)?;
    let target_len = i64::try_from(target_len).map_err(|_| SyncError::SignalTooLong)?;

    if index < target_len {
        Ok(index)
    } else if index >= fft_len - (reference_len - 1) {
        Ok(index - fft_len)
    } else {
        Err(SyncError::NoPeak)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE_HZ: u32 = 48_000;

    fn rate() -> SampleRate {
        SampleRate::new(RATE_HZ).unwrap()
    }

    /// `len` samples of silence with `pattern` written starting at `at`.
    fn with_pattern(len: usize, at: usize, pattern: &[f32]) -> Vec<f32> {
        let mut signal = vec![0.0; len];
        for (offset, &value) in pattern.iter().enumerate() {
            if let Some(slot) = signal.get_mut(at + offset) {
                *slot = value;
            }
        }
        signal
    }

    const PATTERN: [f32; 4] = [0.2, 0.9, -0.5, 0.3];

    #[test]
    fn recovers_a_positive_delay() {
        // Same feature, 30 samples later in the target -> target lags by 30.
        let reference = with_pattern(1_000, 100, &PATTERN);
        let target = with_pattern(1_000, 130, &PATTERN);

        let offset = align(
            &reference,
            &target,
            rate(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(offset.samples(), 30);
        // Concrete rational so a broken conversion cannot pass: 30 / 48000 s.
        assert_eq!(offset.seconds(), Seconds::new(30, 48_000).unwrap());
    }

    #[test]
    fn recovers_a_negative_delay() {
        // Feature earlier in the target -> target leads -> negative offset.
        let reference = with_pattern(1_000, 130, &PATTERN);
        let target = with_pattern(1_000, 100, &PATTERN);

        let offset = align(
            &reference,
            &target,
            rate(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(offset.samples(), -30);
    }

    #[test]
    fn identical_signals_have_zero_offset() {
        let signal = with_pattern(1_000, 200, &PATTERN);
        let offset = align(
            &signal,
            &signal,
            rate(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(offset.samples(), 0);
    }

    #[test]
    fn differing_lengths_align() {
        // A short target located inside a longer reference.
        let reference = with_pattern(2_000, 500, &PATTERN);
        let target = with_pattern(300, 0, &PATTERN);

        // reference feature at 500, target feature at 0 -> target leads by 500.
        let offset = align(
            &reference,
            &target,
            rate(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(offset.samples(), -500);
    }

    #[test]
    fn recovers_large_positive_lag_with_short_reference() {
        // Short reference, long target, feature near the target's end: the lag
        // exceeds fft_len/2, which a fixed-midpoint decode would mis-sign.
        let reference = with_pattern(10, 0, &PATTERN);
        let target = with_pattern(1_000, 990, &PATTERN);

        let offset = align(
            &reference,
            &target,
            rate(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(offset.samples(), 990);
    }

    #[test]
    fn recovers_large_negative_lag_with_short_target() {
        // The mirror case: long reference, short target, large negative lag.
        let reference = with_pattern(1_000, 990, &PATTERN);
        let target = with_pattern(10, 0, &PATTERN);

        let offset = align(
            &reference,
            &target,
            rate(),
            CorrelationMethod::CrossCorrelation,
        )
        .unwrap();
        assert_eq!(offset.samples(), -990);
    }

    #[test]
    fn silent_signals_have_no_peak() {
        let silence = vec![0.0_f32; 1_000];
        assert!(matches!(
            align(
                &silence,
                &silence,
                rate(),
                CorrelationMethod::CrossCorrelation
            ),
            Err(SyncError::NoPeak)
        ));
    }

    #[test]
    fn empty_reference_is_an_error() {
        let target = with_pattern(100, 0, &PATTERN);
        assert!(matches!(
            align(&[], &target, rate(), CorrelationMethod::CrossCorrelation),
            Err(SyncError::EmptySignal)
        ));
    }

    #[test]
    fn empty_target_is_an_error() {
        let reference = with_pattern(100, 0, &PATTERN);
        assert!(matches!(
            align(&reference, &[], rate(), CorrelationMethod::CrossCorrelation),
            Err(SyncError::EmptySignal)
        ));
    }

    #[test]
    fn lag_from_unrepresentable_index_is_an_error() {
        // The one SignalTooLong site testable without allocating: an index that
        // overflows i64. The two length-arithmetic sites need usize::MAX-sized
        // buffers and rely on the checked operators instead.
        assert!(matches!(
            lag_from_index(usize::MAX, usize::MAX, 1, 1),
            Err(SyncError::SignalTooLong)
        ));
    }

    #[test]
    fn phat_recovers_the_same_offsets_as_cross_correlation() {
        // Positive and asymmetric large lags both decode correctly under PHAT.
        let reference = with_pattern(1_000, 100, &PATTERN);
        let target = with_pattern(1_000, 130, &PATTERN);
        assert_eq!(
            align(&reference, &target, rate(), CorrelationMethod::Phat)
                .unwrap()
                .samples(),
            30
        );

        let short_reference = with_pattern(10, 0, &PATTERN);
        let long_target = with_pattern(1_000, 990, &PATTERN);
        assert_eq!(
            align(
                &short_reference,
                &long_target,
                rate(),
                CorrelationMethod::Phat
            )
            .unwrap()
            .samples(),
            990
        );
    }

    #[test]
    fn phat_offset_is_amplitude_invariant() {
        // Whitening removes magnitude, so scaling a signal cannot change the
        // result — the offset is identical regardless of the target's gain.
        let reference = with_pattern(1_000, 100, &PATTERN);
        let quiet = with_pattern(1_000, 130, &PATTERN);
        let loud: Vec<f32> = quiet.iter().map(|&value| value * 1_000.0).collect();

        let quiet_offset = align(&reference, &quiet, rate(), CorrelationMethod::Phat).unwrap();
        let loud_offset = align(&reference, &loud, rate(), CorrelationMethod::Phat).unwrap();

        assert_eq!(quiet_offset, loud_offset);
        assert_eq!(quiet_offset.samples(), 30);
    }

    #[test]
    fn phat_silent_signals_have_no_peak() {
        let silence = vec![0.0_f32; 1_000];
        assert!(matches!(
            align(&silence, &silence, rate(), CorrelationMethod::Phat),
            Err(SyncError::NoPeak)
        ));
    }
}
