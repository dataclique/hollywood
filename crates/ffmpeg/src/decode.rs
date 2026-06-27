//! Audio decoding to mono sample buffers, behind a backend-swappable trait.
//!
//! Detection (silence gating, VAD) and synchronization (cross-correlation) work
//! on raw audio, so they need the source decoded into a flat mono buffer.
//! [`FfmpegDecoder`] decodes the best audio stream, converts each frame to
//! planar `f32` via FFmpeg's resampler, and averages the channels itself —
//! which, unlike FFmpeg's energy-preserving mono downmix, keeps samples in
//! `[-1.0, 1.0]` — yielding samples at the source's native rate. The
//! [`DecodeAudio`] trait keeps callers backend-agnostic so a pure-Rust backend
//! (Symphonia) can replace it.
//!
//! The whole signal is decoded into one buffer, so a long take is large (≈2 GB
//! for three hours of 48 kHz mono); a chunked/windowed API is a follow-up before
//! `hollywood-detect`/`hollywood-sync` consume long recordings.
//!
//! As with probing, [`ffmpeg_next::init`] must run once at process startup
//! before any decode.

use std::path::Path;

use ffmpeg_next::ChannelLayout;
use ffmpeg_next::Error as FfmpegError;
use ffmpeg_next::codec::context::Context;
use ffmpeg_next::decoder;
use ffmpeg_next::format;
use ffmpeg_next::format::Sample;
use ffmpeg_next::format::sample::Type as SampleType;
use ffmpeg_next::frame::Audio;
use ffmpeg_next::media::Type;
use ffmpeg_next::software::resampling;
use ffmpeg_next::util::error::EAGAIN;

use hollywood_timeline::SampleRate;

use crate::error::MediaError;

/// A decoded audio signal downmixed to a single channel.
#[derive(Clone, Debug, PartialEq)]
pub struct MonoAudio {
    /// Mono samples in `[-1.0, 1.0]`, in presentation order.
    pub samples: Vec<f32>,
    /// The rate the buffer was decoded at — the source's native sample rate.
    pub sample_rate: SampleRate,
}

/// A backend that decodes a media source's audio into a mono buffer.
///
/// [`FfmpegDecoder`] is the default implementation; the trait keeps callers
/// independent of FFmpeg so a pure-Rust backend can replace it.
pub trait DecodeAudio {
    /// Decode the best audio stream of the media at `path` into mono samples.
    fn decode_mono(&self, path: &Path) -> Result<MonoAudio, MediaError>;
}

/// An FFmpeg-backed [`DecodeAudio`].
#[derive(Clone, Copy, Debug, Default)]
pub struct FfmpegDecoder;

impl DecodeAudio for FfmpegDecoder {
    fn decode_mono(&self, path: &Path) -> Result<MonoAudio, MediaError> {
        let mut input = format::input(path)?;

        // Scope the stream borrow so it ends before `input.packets()` reborrows.
        let (stream_index, mut decoder) = {
            let stream = input
                .streams()
                .best(Type::Audio)
                .ok_or(MediaError::NoAudioStream)?;
            let decoder = Context::from_parameters(stream.parameters())?
                .decoder()
                .audio()?;
            (stream.index(), decoder)
        };

        let sample_rate = SampleRate::new(decoder.rate())?;

        // The resampler is built from the first decoded frame, not the decoder:
        // a decoder reports no sample format until it has produced a frame, so
        // configuring the converter up front would mismatch the real frames.
        let mut resampler: Option<resampling::Context> = None;
        let mut samples = Vec::new();
        for (stream, packet) in input.packets() {
            if stream.index() == stream_index {
                decoder.send_packet(&packet)?;
                receive_frames(&mut decoder, &mut resampler, &mut samples)?;
            }
        }
        // Flush the decoder. The resampler keeps no tail to drain: output rate
        // equals input rate, so the downmix/format conversion is sample-for-
        // sample with no resampling latency.
        decoder.send_eof()?;
        receive_frames(&mut decoder, &mut resampler, &mut samples)?;

        // An audio stream that demuxes/decodes to nothing is a failure, not a
        // silently-empty buffer downstream cross-correlation cannot interpret.
        if samples.is_empty() {
            return Err(MediaError::NoAudioData);
        }

        Ok(MonoAudio {
            samples,
            sample_rate,
        })
    }
}

/// Pull every decoded frame currently available, downmix each to mono through
/// the resampler (built from the first frame), and append the samples.
fn receive_frames(
    decoder: &mut decoder::Audio,
    resampler: &mut Option<resampling::Context>,
    samples: &mut Vec<f32>,
) -> Result<(), MediaError> {
    let mut frame = Audio::empty();
    loop {
        match decoder.receive_frame(&mut frame) {
            Ok(()) => {}
            // EAGAIN means the decoder needs more input; Eof means it is fully
            // drained. Both end this pass cleanly; any other error is a real
            // decode failure that must not be swallowed into a truncated buffer.
            Err(FfmpegError::Other { errno }) if errno == EAGAIN => return Ok(()),
            Err(FfmpegError::Eof) => return Ok(()),
            Err(error) => return Err(error.into()),
        }

        // Raw PCM frames often carry no channel layout; assign a standard one
        // for the channel count so each frame matches the resampler's input.
        if frame.channel_layout().is_empty() {
            frame.set_channel_layout(ChannelLayout::default(i32::from(frame.channels())));
        }
        let resampler = match resampler.as_mut() {
            Some(existing) => existing,
            None => resampler.insert(f32_converter(&frame)?),
        };
        let mut converted = Audio::empty();
        resampler.run(&frame, &mut converted)?;
        append_downmixed(&converted, samples)?;
    }
}

/// A converter that turns `frame` into planar `f32` while keeping its channels
/// and rate (no resampling, no channel mixing). Built from the frame's own
/// layout (the caller has already given a layout-less frame a standard one), so
/// its input matches. The channel downmix is done in [`append_downmixed`] by
/// averaging, which — unlike FFmpeg's energy-preserving mono downmix — keeps
/// samples within `[-1.0, 1.0]`.
fn f32_converter(frame: &Audio) -> Result<resampling::Context, MediaError> {
    let layout = frame.channel_layout();
    Ok(resampling::Context::get(
        frame.format(),
        layout,
        frame.rate(),
        Sample::F32(SampleType::Planar),
        layout,
        frame.rate(),
    )?)
}

/// Average a planar-`f32` frame's channels into mono samples. Planar layout
/// gives one plane of exactly `samples()` values per channel, so the mean of
/// values already in `[-1.0, 1.0]` stays in range. An empty frame adds none.
fn append_downmixed(frame: &Audio, samples: &mut Vec<f32>) -> Result<(), MediaError> {
    let count = frame.samples();
    let channel_count = frame.channels();
    let channels = usize::from(channel_count);
    if count == 0 || channels == 0 {
        return Ok(());
    }
    let planes: Vec<&[f32]> = (0..channels)
        .filter_map(|channel| frame.plane::<f32>(channel).get(..count))
        .collect();
    // `f32_converter` requests a planar frame, so every channel must expose a
    // full-length plane; a shorter or packed frame is a contract violation, not
    // something to drop silently.
    if planes.len() != channels {
        return Err(MediaError::UnexpectedAudioLayout);
    }
    // `u16 -> f32` is exact, so the mean carries no cast precision loss.
    let scale = 1.0 / f32::from(channel_count);
    for index in 0..count {
        let sum: f32 = planes
            .iter()
            .filter_map(|plane| plane.get(index))
            .copied()
            .sum();
        samples.push(sum * scale);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;

    use super::*;

    /// Write a PCM `s16le` WAV so the real FFmpeg demux/decode path runs.
    fn write_wav(path: &Path, channels: u16, rate: u32, interleaved: &[i16]) {
        let bits_per_sample: u16 = 16;
        let block_align = channels * (bits_per_sample / 8);
        let byte_rate = rate * u32::from(block_align);
        let data_len = u32::try_from(interleaved.len() * 2).unwrap();

        let mut buf = Vec::new();
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&(36 + data_len).to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
        buf.extend_from_slice(&channels.to_le_bytes());
        buf.extend_from_slice(&rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_len.to_le_bytes());
        for sample in interleaved {
            buf.extend_from_slice(&sample.to_le_bytes());
        }
        File::create(path).unwrap().write_all(&buf).unwrap();
    }

    #[test]
    fn decodes_stereo_wav_to_averaged_mono() {
        ffmpeg_next::init().unwrap();

        // Asymmetric L/R so only `(L + R) / 2` averaging yields the expected
        // output — a single-channel passthrough or energy-preserving downmix
        // would not. i16 normalizes by 32768.
        let left: [i16; 4] = [16_384, -16_384, 32_767, 0];
        let right: [i16; 4] = [0, 16_384, 0, -16_384];
        let interleaved: Vec<i16> = left.iter().zip(right).flat_map(|(&l, r)| [l, r]).collect();

        let path: PathBuf = std::env::temp_dir().join("hollywood_decode_stereo.wav");
        write_wav(&path, 2, 8_000, &interleaved);

        let decoded = FfmpegDecoder.decode_mono(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(decoded.sample_rate, SampleRate::new(8_000).unwrap());
        assert_eq!(decoded.samples.len(), 4);
        // Channel means: 0.25, 0.0, ~0.5, -0.25.
        let expected = [0.25, 0.0, 0.5, -0.25];
        for (got, want) in decoded.samples.iter().zip(expected) {
            assert!((got - want).abs() < 1e-3, "got {got}, want {want}");
        }
    }

    #[test]
    fn decoding_non_media_is_an_error() {
        ffmpeg_next::init().unwrap();

        // A non-media file must surface an error, never `Ok` with empty audio —
        // a truncated/empty buffer would silently corrupt downstream analysis.
        let path: PathBuf = std::env::temp_dir().join("hollywood_decode_garbage.bin");
        File::create(&path)
            .unwrap()
            .write_all(b"not a media file")
            .unwrap();

        let result = FfmpegDecoder.decode_mono(&path);
        let _ = std::fs::remove_file(&path);

        assert!(result.is_err());
    }

    #[test]
    fn decodes_mono_wav_unchanged() {
        ffmpeg_next::init().unwrap();

        let frames: [i16; 3] = [0, 16_384, -32_768];
        let path: PathBuf = std::env::temp_dir().join("hollywood_decode_mono.wav");
        write_wav(&path, 1, 16_000, &frames);

        let decoded = FfmpegDecoder.decode_mono(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(decoded.sample_rate, SampleRate::new(16_000).unwrap());
        assert_eq!(decoded.samples.len(), 3);
        let expected = [0.0, 0.5, -1.0];
        for (got, want) in decoded.samples.iter().zip(expected) {
            assert!((got - want).abs() < 1e-3, "got {got}, want {want}");
        }
    }
}
