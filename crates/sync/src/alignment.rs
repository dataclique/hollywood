//! FFT cross-correlation alignment.
//!
//! The cross-correlation of two signals peaks at the lag that best aligns them.
//! Computed via the convolution theorem: `IFFT(conj(R) · T)` where `R`, `T` are
//! the signals' spectra. Both signals are zero-padded to at least their combined
//! length so the circular correlation the FFT yields equals the linear one, and
//! the peak's index maps to the lag (indices past the midpoint are negative
//! lags that wrapped around).

use hollywood_timeline::{SampleRate, Seconds};
use realfft::RealToComplex;
use realfft::num_complex::Complex;

use crate::error::SyncError;

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

/// Align `target` to `reference` (both mono, at `rate`) by cross-correlation,
/// returning how far `target` lags `reference`.
///
/// # Errors
///
/// [`SyncError::EmptySignal`] if either signal is empty,
/// [`SyncError::SignalTooLong`] if their combined length is unrepresentable,
/// [`SyncError::Fft`] if the transform fails, and [`SyncError::NoPeak`] if no
/// correlation peak is found.
pub fn align(reference: &[f32], target: &[f32], rate: SampleRate) -> Result<SyncOffset, SyncError> {
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

    // Cross-power spectrum conj(R)·T; its inverse transform is the correlation.
    let mut cross = forward.make_output_vec();
    for ((slot, &r), &t) in cross
        .iter_mut()
        .zip(&reference_spectrum)
        .zip(&target_spectrum)
    {
        *slot = r.conj() * t;
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
    Ok(SyncOffset {
        samples: lag_from_index(peak, fft_len)?,
        rate,
    })
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

/// Map a correlation-buffer index to a signed lag. Indices in the second half
/// represent negative lags that wrapped around the circular correlation.
fn lag_from_index(index: usize, fft_len: usize) -> Result<i64, SyncError> {
    let index = i64::try_from(index).map_err(|_| SyncError::SignalTooLong)?;
    let len = i64::try_from(fft_len).map_err(|_| SyncError::SignalTooLong)?;
    Ok(if index <= len / 2 { index } else { index - len })
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

        let offset = align(&reference, &target, rate()).unwrap();
        assert_eq!(offset.samples(), 30);
        assert_eq!(offset.seconds(), Seconds::from_samples(30, rate()));
    }

    #[test]
    fn recovers_a_negative_delay() {
        // Feature earlier in the target -> target leads -> negative offset.
        let reference = with_pattern(1_000, 130, &PATTERN);
        let target = with_pattern(1_000, 100, &PATTERN);

        let offset = align(&reference, &target, rate()).unwrap();
        assert_eq!(offset.samples(), -30);
    }

    #[test]
    fn identical_signals_have_zero_offset() {
        let signal = with_pattern(1_000, 200, &PATTERN);
        let offset = align(&signal, &signal, rate()).unwrap();
        assert_eq!(offset.samples(), 0);
    }

    #[test]
    fn differing_lengths_align() {
        // A short target located inside a longer reference.
        let reference = with_pattern(2_000, 500, &PATTERN);
        let target = with_pattern(300, 0, &PATTERN);

        // reference feature at 500, target feature at 0 -> target leads by 500.
        let offset = align(&reference, &target, rate()).unwrap();
        assert_eq!(offset.samples(), -500);
    }

    #[test]
    fn empty_reference_is_an_error() {
        let target = with_pattern(100, 0, &PATTERN);
        assert!(matches!(
            align(&[], &target, rate()),
            Err(SyncError::EmptySignal)
        ));
    }

    #[test]
    fn empty_target_is_an_error() {
        let reference = with_pattern(100, 0, &PATTERN);
        assert!(matches!(
            align(&reference, &[], rate()),
            Err(SyncError::EmptySignal)
        ));
    }
}
