//! Golden-file tests for the FCPXML exporter. The checked-in golden under
//! `tests/golden/` is the regression contract; a mismatch fails the test.
//!
//! NOTE: like the xmeml goldens, these are produced by this exporter, not
//! exported from a real Final Cut / Resolve, so they pin *regressions* but do
//! not yet prove the output imports. To regenerate after an intentional change,
//! run with `UPDATE_GOLDENS=1`; a missing golden fails loudly rather than being
//! silently minted.

use std::path::PathBuf;

use hollywood_nle::{NleError, to_fcpxml};
use hollywood_timeline::{
    AudioProperties, ChannelLayout, Clip, FrameRate, Gap, MediaAsset, MediaSource, SampleRate,
    Seconds, TimeRange, Timeline, TimelineError, Track, TrackKind, Transition, VideoProperties,
};

/// A timeline with a primary video track (two clips around a gap) and a
/// connected audio track — exercising format dedup, explicit audio channels,
/// and a connected clip on a negative lane. Returns `Result` so the shared
/// fixture stays `unwrap`-free; the `#[test]`s unwrap it.
fn sample_timeline() -> Result<Timeline, TimelineError> {
    let fps = FrameRate::whole(30)?;
    let video = VideoProperties {
        frame_rate: fps,
        width: 1920,
        height: 1080,
    };
    let stereo = AudioProperties {
        sample_rate: SampleRate::new(48_000)?,
        channels: ChannelLayout::Stereo,
    };

    let mut timeline = Timeline::new("demo", fps);
    for id in ["a.mov", "b.mov"] {
        timeline.add_asset(MediaAsset::new(
            MediaSource::file(id),
            Seconds::from_secs(60),
            Some(video),
            None,
        )?)?;
    }
    timeline.add_asset(MediaAsset::new(
        MediaSource::file("vo.wav"),
        Seconds::from_secs(60),
        None,
        Some(stereo),
    )?)?;

    let mut video_track = Track::new(TrackKind::Video);
    video_track.push_clip(Clip::with_name(
        MediaSource::file("a.mov"),
        TimeRange::new(Seconds::from_secs(2), Seconds::from_secs(3))?,
        "intro",
    ));
    video_track.push_gap(Gap::new(Seconds::from_secs(1))?)?;
    video_track.push_clip(Clip::with_name(
        MediaSource::file("b.mov"),
        TimeRange::new(Seconds::from_secs(10), Seconds::from_secs(4))?,
        "outro",
    ));
    timeline.add_track(video_track);

    let mut audio_track = Track::new(TrackKind::Audio);
    audio_track.push_clip(Clip::with_name(
        MediaSource::file("vo.wav"),
        TimeRange::new(Seconds::ZERO, Seconds::from_secs(8))?,
        "voice",
    ));
    timeline.add_track(audio_track);

    timeline.validate()?;
    Ok(timeline)
}

#[test]
fn video_and_audio_tracks_match_golden() {
    let timeline = sample_timeline().unwrap();
    assert_golden(
        "video_and_audio_tracks.fcpxml",
        &to_fcpxml(&timeline).unwrap(),
    )
    .unwrap();
}

#[test]
fn audio_asset_declares_explicit_channel_sources() {
    let xml = to_fcpxml(&sample_timeline().unwrap()).unwrap();
    // The audio asset carries explicit channel sourcing, not just a stream flag.
    assert!(xml.contains(r#"hasAudio="1""#));
    assert!(xml.contains(r#"audioSources="1""#));
    assert!(xml.contains(r#"audioChannels="2""#));
    assert!(xml.contains(r#"audioRate="48000""#));
}

#[test]
fn connected_audio_track_uses_a_negative_lane() {
    let xml = to_fcpxml(&sample_timeline().unwrap()).unwrap();
    // The non-primary audio track composites below the video primary.
    assert!(xml.contains(r#"lane="-1""#));
    assert!(xml.contains("<spine>"));
    // Rational frame-duration time, not floating seconds.
    assert!(xml.contains(r#"frameDuration="1/30s""#));
}

#[test]
fn video_assets_with_the_same_geometry_share_one_format() {
    let xml = to_fcpxml(&sample_timeline().unwrap()).unwrap();
    // a.mov and b.mov are both 1920x1080@30, so they share the single format r1
    // (the sequence format) — there is exactly one <format> element.
    assert_eq!(xml.matches("<format ").count(), 1);
}

#[test]
fn transitions_are_rejected_for_now() {
    let stereo = AudioProperties {
        sample_rate: SampleRate::new(48_000).unwrap(),
        channels: ChannelLayout::Stereo,
    };
    let mut timeline = Timeline::new("demo", FrameRate::whole(30).unwrap());
    timeline
        .add_asset(
            MediaAsset::new(
                MediaSource::file("a.wav"),
                Seconds::from_secs(60),
                None,
                Some(stereo),
            )
            .unwrap(),
        )
        .unwrap();
    let mut track = Track::new(TrackKind::Audio);
    track.push_clip(Clip::new(
        MediaSource::file("a.wav"),
        TimeRange::new(Seconds::ZERO, Seconds::from_secs(2)).unwrap(),
    ));
    track
        .push_transition(Transition::cross_fade(Seconds::from_secs(1)).unwrap())
        .unwrap();
    track.push_clip(Clip::new(
        MediaSource::file("a.wav"),
        TimeRange::new(Seconds::from_secs(3), Seconds::from_secs(2)).unwrap(),
    ));
    timeline.add_track(track);

    assert!(matches!(
        to_fcpxml(&timeline),
        Err(NleError::UnsupportedTransition)
    ));
}

#[test]
fn sub_frame_duration_is_rejected() {
    let video = VideoProperties {
        frame_rate: FrameRate::whole(30).unwrap(),
        width: 1920,
        height: 1080,
    };
    let mut timeline = Timeline::new("demo", FrameRate::whole(30).unwrap());
    timeline
        .add_asset(
            MediaAsset::new(
                MediaSource::file("a.mov"),
                Seconds::from_secs(60),
                Some(video),
                None,
            )
            .unwrap(),
        )
        .unwrap();
    let mut track = Track::new(TrackKind::Video);
    // 1/7 s is not a whole number of frames at 30 fps.
    track.push_clip(Clip::new(
        MediaSource::file("a.mov"),
        TimeRange::new(Seconds::ZERO, Seconds::new(1, 7).unwrap()).unwrap(),
    ));
    timeline.add_track(track);
    timeline.validate().unwrap();

    assert!(matches!(
        to_fcpxml(&timeline),
        Err(NleError::UnalignedDuration)
    ));
}

#[test]
fn ntsc_frame_rate_is_rejected() {
    let rate = FrameRate::new(30_000, 1001).unwrap();
    let video = VideoProperties {
        frame_rate: rate,
        width: 1920,
        height: 1080,
    };
    let mut timeline = Timeline::new("demo", rate);
    timeline
        .add_asset(
            MediaAsset::new(
                MediaSource::file("a.mov"),
                Seconds::from_secs(60),
                Some(video),
                None,
            )
            .unwrap(),
        )
        .unwrap();
    let mut track = Track::new(TrackKind::Video);
    track.push_clip(Clip::new(
        MediaSource::file("a.mov"),
        TimeRange::new(Seconds::ZERO, Seconds::from_secs(1)).unwrap(),
    ));
    timeline.add_track(track);
    timeline.validate().unwrap();

    assert!(matches!(
        to_fcpxml(&timeline),
        Err(NleError::UnsupportedFrameRate)
    ));
}

/// An audio-only timeline (no video asset) — exercises the
/// `DEFAULT_WIDTH`/`DEFAULT_HEIGHT` fallback for the sequence format and the
/// audio-asset path that carries no `format` ref. Returns `Result` so the
/// fixture stays `unwrap`-free; the `#[test]`s unwrap it.
fn sample_audio_only_timeline() -> Result<Timeline, TimelineError> {
    let stereo = AudioProperties {
        sample_rate: SampleRate::new(48_000)?,
        channels: ChannelLayout::Stereo,
    };
    let mut timeline = Timeline::new("podcast", FrameRate::whole(30)?);
    timeline.add_asset(MediaAsset::new(
        MediaSource::file("vo.wav"),
        Seconds::from_secs(60),
        None,
        Some(stereo),
    )?)?;
    let mut audio = Track::new(TrackKind::Audio);
    audio.push_clip(Clip::with_name(
        MediaSource::file("vo.wav"),
        TimeRange::new(Seconds::ZERO, Seconds::from_secs(5))?,
        "voice",
    ));
    timeline.add_track(audio);
    timeline.validate()?;
    Ok(timeline)
}

#[test]
fn audio_only_timeline_matches_golden() {
    let timeline = sample_audio_only_timeline().unwrap();
    assert_golden("audio_only.fcpxml", &to_fcpxml(&timeline).unwrap()).unwrap();
}

#[test]
fn audio_only_timeline_uses_default_format_and_omits_video() {
    let xml = to_fcpxml(&sample_audio_only_timeline().unwrap()).unwrap();
    // No video asset -> the sequence format falls back to 1920x1080.
    assert!(xml.contains(r#"width="1920""#));
    assert!(xml.contains(r#"height="1080""#));
    // The audio asset declares no video stream (and carries no `format` ref).
    assert!(xml.contains(r#"hasVideo="0""#));
    assert!(!xml.contains(r#"hasVideo="1""#));
}

#[test]
fn connected_audio_offset_carries_the_host_clip_in_point() {
    // The primary video clip is trimmed to a 2s in-point (start=60/30s). A
    // connected clip's offset is in the host's local time, so the voice clip
    // anchored to it must be emitted at that in-point (60/30s), which an
    // importer resolves back to sequence time 0s — not the naive 0s offset that
    // would resolve to -2s.
    let xml = to_fcpxml(&sample_timeline().unwrap()).unwrap();
    assert!(xml.contains(r#"<asset-clip ref="r4" lane="-1" offset="60/30s" name="voice""#));
}

/// Compare `actual` against the checked-in golden. A missing golden returns an
/// error (which the caller surfaces) rather than being silently minted; pass
/// `UPDATE_GOLDENS=1` to (re)write it after an intentional change.
fn assert_golden(name: &str, actual: &str) -> std::io::Result<()> {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "golden", name]
        .iter()
        .collect();

    if std::env::var_os("UPDATE_GOLDENS").is_some() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        return std::fs::write(&path, actual);
    }

    let expected = std::fs::read_to_string(&path)?;
    assert_eq!(
        actual,
        expected.as_str(),
        "golden {name} mismatch (run UPDATE_GOLDENS=1 to regenerate)"
    );
    Ok(())
}
