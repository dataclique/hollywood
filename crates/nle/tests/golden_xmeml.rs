//! Golden-file tests for the xmeml exporter. The checked-in golden under
//! `tests/golden/` is the regression contract; a mismatch fails the test.
//!
//! NOTE: the committed goldens are currently produced by this exporter, not
//! exported from a real NLE, so they pin *regressions* but do not yet prove the
//! output imports into Premiere/Resolve. Validating a golden against a real
//! Premiere/DaVinci export is tracked follow-up work. To regenerate after an
//! intentional change, run with `UPDATE_GOLDENS=1`; a missing golden fails
//! loudly rather than being silently minted.

use std::path::PathBuf;

use hollywood_nle::{NleError, to_xmeml};
use hollywood_timeline::{
    AudioProperties, ChannelLayout, Clip, FrameRate, Gap, MediaAsset, MediaSource, SampleRate,
    Seconds, TimeRange, Timeline, Track, TrackKind, Transition, VideoProperties,
};

#[test]
fn two_clip_video_track_matches_golden() {
    let mut timeline = Timeline::new("demo", FrameRate::whole(30).unwrap());
    let video = VideoProperties {
        frame_rate: FrameRate::whole(30).unwrap(),
        width: 1920,
        height: 1080,
    };
    for id in ["a.mov", "b.mov"] {
        timeline
            .add_asset(
                MediaAsset::new(
                    MediaSource::file(id),
                    Seconds::from_secs(60),
                    Some(video),
                    None,
                )
                .unwrap(),
            )
            .unwrap();
    }

    let mut track = Track::new(TrackKind::Video);
    track.push_clip(Clip::with_name(
        MediaSource::file("a.mov"),
        TimeRange::new(Seconds::from_secs(2), Seconds::from_secs(3)).unwrap(),
        "intro",
    ));
    track
        .push_gap(Gap::new(Seconds::from_secs(1)).unwrap())
        .unwrap();
    track.push_clip(Clip::with_name(
        MediaSource::file("b.mov"),
        TimeRange::new(Seconds::from_secs(10), Seconds::from_secs(4)).unwrap(),
        "outro",
    ));
    timeline.add_track(track);
    timeline.validate().unwrap();

    assert_golden("two_clip_video_track.xml", &to_xmeml(&timeline).unwrap()).unwrap();
}

/// An audio track joining two takes with a one-second cross-fade. The fade
/// overlays the cut between the takes (it does not advance the track), so the
/// exporter must emit a centered `transitionitem` between the two clipitems.
fn cross_fade_timeline() -> Result<Timeline, hollywood_timeline::TimelineError> {
    let stereo = AudioProperties {
        sample_rate: SampleRate::new(48_000)?,
        channels: ChannelLayout::Stereo,
    };
    let mut timeline = Timeline::new("demo", FrameRate::whole(30)?);
    for id in ["vo1.wav", "vo2.wav"] {
        timeline.add_asset(MediaAsset::new(
            MediaSource::file(id),
            Seconds::from_secs(60),
            None,
            Some(stereo),
        )?)?;
    }

    let mut track = Track::new(TrackKind::Audio);
    // take-1 occupies timeline [0s, 2s]; it has media after its out-point to
    // feed the second half of the fade.
    track.push_clip(Clip::with_name(
        MediaSource::file("vo1.wav"),
        TimeRange::new(Seconds::ZERO, Seconds::from_secs(2))?,
        "take-1",
    ));
    track.push_transition(Transition::cross_fade(Seconds::from_secs(1))?)?;
    // take-2 is trimmed to a 3s in-point, so it has media before its in-point to
    // feed the first half of the fade.
    track.push_clip(Clip::with_name(
        MediaSource::file("vo2.wav"),
        TimeRange::new(Seconds::from_secs(3), Seconds::from_secs(2))?,
        "take-2",
    ));
    timeline.add_track(track);
    timeline.validate()?;
    Ok(timeline)
}

#[test]
fn audio_cross_fade_matches_golden() {
    let timeline = cross_fade_timeline().unwrap();
    assert_golden("audio_cross_fade.xml", &to_xmeml(&timeline).unwrap()).unwrap();
}

#[test]
fn audio_cross_fade_emits_a_centered_transitionitem() {
    let xml = to_xmeml(&cross_fade_timeline().unwrap()).unwrap();
    // The cut sits at 2s (60 frames at 30 fps); a 1s (30-frame) centered fade
    // spans 60 ± 15 frames, i.e. [45, 75].
    assert!(xml.contains("<transitionitem>"));
    assert!(xml.contains("<alignment>center</alignment>"));
    assert!(xml.contains("<start>45</start>"));
    assert!(xml.contains("<end>75</end>"));
    // The default audio dissolve is the constant-power (+3 dB) cross-fade.
    assert!(xml.contains("<effectid>KGAudioTransCrossFade3dB</effectid>"));
    assert!(xml.contains("<mediatype>audio</mediatype>"));
}

#[test]
fn odd_frame_cross_fade_centers_as_closely_as_the_timebase_allows() {
    // A 0.5s fade is 15 frames at 30 fps — an odd span that cannot split
    // evenly. The cut sits at 2s (60 frames); lead = 15 / 2 = 7 frames precede
    // it and 8 follow, so the transition spans [53, 68] (end - start == 15).
    // The clips still abut at frame 60, which keeps `center` faithful.
    let stereo = AudioProperties {
        sample_rate: SampleRate::new(48_000).unwrap(),
        channels: ChannelLayout::Stereo,
    };
    let mut timeline = Timeline::new("demo", FrameRate::whole(30).unwrap());
    for id in ["vo1.wav", "vo2.wav"] {
        timeline
            .add_asset(
                MediaAsset::new(
                    MediaSource::file(id),
                    Seconds::from_secs(60),
                    None,
                    Some(stereo),
                )
                .unwrap(),
            )
            .unwrap();
    }
    let mut track = Track::new(TrackKind::Audio);
    track.push_clip(Clip::new(
        MediaSource::file("vo1.wav"),
        TimeRange::new(Seconds::ZERO, Seconds::from_secs(2)).unwrap(),
    ));
    track
        .push_transition(Transition::cross_fade(Seconds::new(1, 2).unwrap()).unwrap())
        .unwrap();
    track.push_clip(Clip::new(
        MediaSource::file("vo2.wav"),
        TimeRange::new(Seconds::from_secs(3), Seconds::from_secs(2)).unwrap(),
    ));
    timeline.add_track(track);
    timeline.validate().unwrap();

    let xml = to_xmeml(&timeline).unwrap();
    assert!(xml.contains("<start>53</start>"));
    assert!(xml.contains("<end>68</end>"));
    assert!(xml.contains("<alignment>center</alignment>"));
}

#[test]
fn sub_frame_cross_fade_is_rejected() {
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
    // 1/7 s is not a whole number of frames at 30 fps, so the fade boundary is
    // not frame-aligned and the exporter must refuse rather than snap it.
    track
        .push_transition(Transition::cross_fade(Seconds::new(1, 7).unwrap()).unwrap())
        .unwrap();
    track.push_clip(Clip::new(
        MediaSource::file("a.wav"),
        TimeRange::new(Seconds::from_secs(3), Seconds::from_secs(2)).unwrap(),
    ));
    timeline.add_track(track);
    timeline.validate().unwrap();

    assert!(matches!(
        to_xmeml(&timeline),
        Err(NleError::UnalignedDuration)
    ));
}

#[test]
fn sub_frame_duration_is_rejected() {
    let mut timeline = Timeline::new("demo", FrameRate::whole(30).unwrap());
    let video = VideoProperties {
        frame_rate: FrameRate::whole(30).unwrap(),
        width: 1920,
        height: 1080,
    };
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
    // 1/7 s is not a whole number of frames at 30 fps, so the exporter must
    // refuse to snap it rather than emit a misaligned clip.
    track.push_clip(Clip::new(
        MediaSource::file("a.mov"),
        TimeRange::new(Seconds::ZERO, Seconds::new(1, 7).unwrap()).unwrap(),
    ));
    timeline.add_track(track);
    timeline.validate().unwrap();

    assert!(matches!(
        to_xmeml(&timeline),
        Err(NleError::UnalignedDuration)
    ));
}

#[test]
fn ntsc_frame_rate_is_rejected() {
    let rate = FrameRate::new(30_000, 1001).unwrap();
    let mut timeline = Timeline::new("demo", rate);
    let video = VideoProperties {
        frame_rate: rate,
        width: 1920,
        height: 1080,
    };
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

    // 29.97 fps (30000/1001) is not a whole timebase; the exporter refuses it
    // rather than emit incorrect integer frame numbers.
    assert!(matches!(
        to_xmeml(&timeline),
        Err(NleError::UnsupportedFrameRate)
    ));
}

#[test]
fn audio_track_is_serialized() {
    let mut timeline = Timeline::new("demo", FrameRate::whole(30).unwrap());
    timeline
        .add_asset(
            MediaAsset::new(
                MediaSource::file("v.wav"),
                Seconds::from_secs(60),
                None,
                Some(AudioProperties {
                    sample_rate: SampleRate::new(48_000).unwrap(),
                    channels: ChannelLayout::Stereo,
                }),
            )
            .unwrap(),
        )
        .unwrap();
    let mut track = Track::new(TrackKind::Audio);
    track.push_clip(Clip::with_name(
        MediaSource::file("v.wav"),
        TimeRange::new(Seconds::ZERO, Seconds::from_secs(2)).unwrap(),
        "voice",
    ));
    timeline.add_track(track);
    timeline.validate().unwrap();

    let xml = to_xmeml(&timeline).unwrap();
    // The audio path emits an <audio> media section carrying the clip, and an
    // audio-only timeline emits no <video> section.
    assert!(xml.contains("<audio>") && xml.contains("</audio>"));
    assert!(xml.contains("<name>voice</name>"));
    assert!(xml.contains("<pathurl>v.wav</pathurl>"));
    assert!(!xml.contains("<video>"));
}

#[test]
fn trailing_sub_frame_gap_is_rejected() {
    let mut timeline = Timeline::new("demo", FrameRate::whole(30).unwrap());
    let video = VideoProperties {
        frame_rate: FrameRate::whole(30).unwrap(),
        width: 1920,
        height: 1080,
    };
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
    // A trailing gap of 1/7 s is not frame-aligned at 30 fps and must be
    // rejected even though no clip follows it.
    track
        .push_gap(Gap::new(Seconds::new(1, 7).unwrap()).unwrap())
        .unwrap();
    timeline.add_track(track);
    timeline.validate().unwrap();

    assert!(matches!(
        to_xmeml(&timeline),
        Err(NleError::UnalignedDuration)
    ));
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
