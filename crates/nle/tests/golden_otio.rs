//! Golden-file tests for the OpenTimelineIO (`.otio`) exporter. The checked-in
//! golden under `tests/golden/` is the regression contract; a mismatch fails the
//! test, and it is the sole guard on OTIO-schema conformance — no OpenTimelineIO
//! library or Python runtime is in the tree (ADR 0002), so conformance rests on
//! the reviewed golden, not an automated validator. A separate test parses the
//! output to confirm it is well-formed JSON and walks the expected structure and
//! frame values. To regenerate after an intentional change, run with
//! `UPDATE_GOLDENS=1`; a missing golden fails loudly rather than being silently
//! minted.

use std::path::PathBuf;

use hollywood_nle::{NleError, to_otio};
use hollywood_timeline::{
    AudioProperties, ChannelLayout, Clip, FrameRate, Gap, MediaAsset, MediaSource, SampleRate,
    Seconds, TimeRange, Timeline, TimelineError, Track, TrackKind, Transition, VideoProperties,
};

/// A timeline exercising every track-item kind: a video track of two clips
/// around a gap, and an audio track of two clips joined by a cross-fade. Returns
/// `Result` so the shared fixture stays `unwrap`-free; the `#[test]`s unwrap it.
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
        TimeRange::new(Seconds::ZERO, Seconds::from_secs(2))?,
        "take-1",
    ));
    audio_track.push_transition(Transition::cross_fade(Seconds::from_secs(1))?)?;
    audio_track.push_clip(Clip::with_name(
        MediaSource::file("vo.wav"),
        TimeRange::new(Seconds::from_secs(3), Seconds::from_secs(2))?,
        "take-2",
    ));
    timeline.add_track(audio_track);

    timeline.validate()?;
    Ok(timeline)
}

#[test]
fn timeline_matches_golden() {
    let timeline = sample_timeline().unwrap();
    assert_golden("timeline.otio", &to_otio(&timeline).unwrap()).unwrap();
}

#[test]
fn output_parses_with_the_expected_structure() {
    // The `from_str` parse only confirms well-formed JSON (the exporter builds
    // through serde_json, so it cannot emit invalid JSON); the load-bearing part
    // is walking the parsed tree and asserting the OTIO structure and exact frame
    // values. OTIO-schema conformance itself rests on the reviewed golden.
    let json = to_otio(&sample_timeline().unwrap()).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).expect("output must be valid JSON");

    // The document is an OTIO Timeline whose `tracks` is a single Stack.
    assert_eq!(value["OTIO_SCHEMA"], "Timeline.1");
    assert_eq!(value["name"], "demo");
    assert_eq!(value["tracks"]["OTIO_SCHEMA"], "Stack.1");

    let tracks = value["tracks"]["children"].as_array().unwrap();
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0]["OTIO_SCHEMA"], "Track.1");
    assert_eq!(tracks[0]["kind"], "Video");
    assert_eq!(tracks[1]["kind"], "Audio");

    // The video track lays out clip, gap, clip.
    let video = tracks[0]["children"].as_array().unwrap();
    assert_eq!(video[0]["OTIO_SCHEMA"], "Clip.1");
    assert_eq!(video[1]["OTIO_SCHEMA"], "Gap.1");
    assert_eq!(video[2]["OTIO_SCHEMA"], "Clip.1");

    // The first clip's editorial trim is whole frames at the sequence rate.
    assert_eq!(video[0]["name"], "intro");
    assert_eq!(video[0]["source_range"]["start_time"]["value"], 60);
    assert_eq!(video[0]["source_range"]["duration"]["value"], 90);
    assert_eq!(video[0]["source_range"]["start_time"]["rate"], 30);
    // The media's available_range is the exact source length (60 s = 60/1 s), at
    // the source's own rational rate — not forced onto the 30 fps frame grid.
    let media = &video[0]["media_reference"];
    assert_eq!(media["OTIO_SCHEMA"], "ExternalReference.1");
    assert_eq!(media["target_url"], "a.mov");
    assert_eq!(media["available_range"]["duration"]["value"], 60);
    assert_eq!(media["available_range"]["duration"]["rate"], 1);
}

#[test]
fn cross_fade_becomes_a_transition_between_its_clips() {
    let value: serde_json::Value =
        serde_json::from_str(&to_otio(&sample_timeline().unwrap()).unwrap()).unwrap();
    let audio = value["tracks"]["children"][1]["children"]
        .as_array()
        .unwrap();

    // The cross-fade sits between the two audio clips as a Transition.
    assert_eq!(audio[0]["OTIO_SCHEMA"], "Clip.1");
    assert_eq!(audio[1]["OTIO_SCHEMA"], "Transition.1");
    assert_eq!(audio[2]["OTIO_SCHEMA"], "Clip.1");
    assert_eq!(audio[1]["transition_type"], "SMPTE_Dissolve");
    // A 1s (30-frame) fade reaches 15 frames into each neighbour.
    assert_eq!(audio[1]["in_offset"]["value"], 15);
    assert_eq!(audio[1]["out_offset"]["value"], 15);
}

#[test]
fn odd_frame_cross_fade_splits_the_transition_offsets() {
    // A 3-frame fade (0.1s at 30fps) can't split evenly: lead = 3 / 2 = 1 frame
    // reaches into the preceding clip, the remaining 2 into the following one.
    // This pins the centering convention the even-span golden cannot.
    let stereo = AudioProperties {
        sample_rate: SampleRate::new(48_000).unwrap(),
        channels: ChannelLayout::Stereo,
    };
    let mut timeline = Timeline::new("demo", FrameRate::whole(30).unwrap());
    timeline
        .add_asset(
            MediaAsset::new(
                MediaSource::file("vo.wav"),
                Seconds::from_secs(60),
                None,
                Some(stereo),
            )
            .unwrap(),
        )
        .unwrap();
    let mut track = Track::new(TrackKind::Audio);
    track.push_clip(Clip::new(
        MediaSource::file("vo.wav"),
        TimeRange::new(Seconds::ZERO, Seconds::from_secs(2)).unwrap(),
    ));
    track
        .push_transition(Transition::cross_fade(Seconds::new(3, 30).unwrap()).unwrap())
        .unwrap();
    track.push_clip(Clip::new(
        MediaSource::file("vo.wav"),
        TimeRange::new(Seconds::from_secs(3), Seconds::from_secs(2)).unwrap(),
    ));
    timeline.add_track(track);
    timeline.validate().unwrap();

    let value: serde_json::Value = serde_json::from_str(&to_otio(&timeline).unwrap()).unwrap();
    let transition = &value["tracks"]["children"][0]["children"][1];
    assert_eq!(transition["OTIO_SCHEMA"], "Transition.1");
    assert_eq!(transition["in_offset"]["value"], 1);
    assert_eq!(transition["out_offset"]["value"], 2);
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
        to_otio(&timeline),
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
        to_otio(&timeline),
        Err(NleError::UnsupportedFrameRate)
    ));
}

#[test]
fn non_frame_aligned_source_duration_is_accepted() {
    // A 1.001 s source is 30.03 frames at 30 fps — not frame-aligned. The clip's
    // editorial trim ([0, 1 s] = 30 frames) IS aligned and fits the source, so
    // the export must succeed: the source length is descriptive metadata, emitted
    // exactly (1001/1000 s) rather than rejected for missing the frame grid.
    let stereo = AudioProperties {
        sample_rate: SampleRate::new(48_000).unwrap(),
        channels: ChannelLayout::Stereo,
    };
    let mut timeline = Timeline::new("demo", FrameRate::whole(30).unwrap());
    timeline
        .add_asset(
            MediaAsset::new(
                MediaSource::file("vo.wav"),
                Seconds::new(1001, 1000).unwrap(),
                None,
                Some(stereo),
            )
            .unwrap(),
        )
        .unwrap();
    let mut track = Track::new(TrackKind::Audio);
    track.push_clip(Clip::new(
        MediaSource::file("vo.wav"),
        TimeRange::new(Seconds::ZERO, Seconds::from_secs(1)).unwrap(),
    ));
    timeline.add_track(track);
    timeline.validate().unwrap();

    let value: serde_json::Value = serde_json::from_str(&to_otio(&timeline).unwrap()).unwrap();
    let clip = &value["tracks"]["children"][0]["children"][0];
    let available = &clip["media_reference"]["available_range"]["duration"];
    assert_eq!(available["value"], 1001);
    assert_eq!(available["rate"], 1000);
}

#[test]
fn audio_only_timeline_exports_a_single_audio_track() {
    let stereo = AudioProperties {
        sample_rate: SampleRate::new(48_000).unwrap(),
        channels: ChannelLayout::Stereo,
    };
    let mut timeline = Timeline::new("podcast", FrameRate::whole(30).unwrap());
    timeline
        .add_asset(
            MediaAsset::new(
                MediaSource::file("vo.wav"),
                Seconds::from_secs(60),
                None,
                Some(stereo),
            )
            .unwrap(),
        )
        .unwrap();
    let mut track = Track::new(TrackKind::Audio);
    track.push_clip(Clip::with_name(
        MediaSource::file("vo.wav"),
        TimeRange::new(Seconds::ZERO, Seconds::from_secs(5)).unwrap(),
        "voice",
    ));
    timeline.add_track(track);
    timeline.validate().unwrap();

    let value: serde_json::Value = serde_json::from_str(&to_otio(&timeline).unwrap()).unwrap();
    let tracks = value["tracks"]["children"].as_array().unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0]["kind"], "Audio");
    assert_eq!(tracks[0]["children"][0]["name"], "voice");
}

#[test]
fn clip_without_a_name_falls_back_to_the_source_filename() {
    // No explicit clip name, so the OTIO `name` is the source's file name — not a
    // placeholder default. (A source with no file name omits the field entirely.)
    let video = VideoProperties {
        frame_rate: FrameRate::whole(30).unwrap(),
        width: 1920,
        height: 1080,
    };
    let mut timeline = Timeline::new("demo", FrameRate::whole(30).unwrap());
    timeline
        .add_asset(
            MediaAsset::new(
                MediaSource::file("shot.mov"),
                Seconds::from_secs(60),
                Some(video),
                None,
            )
            .unwrap(),
        )
        .unwrap();
    let mut track = Track::new(TrackKind::Video);
    track.push_clip(Clip::new(
        MediaSource::file("shot.mov"),
        TimeRange::new(Seconds::ZERO, Seconds::from_secs(2)).unwrap(),
    ));
    timeline.add_track(track);
    timeline.validate().unwrap();

    let value: serde_json::Value = serde_json::from_str(&to_otio(&timeline).unwrap()).unwrap();
    assert_eq!(
        value["tracks"]["children"][0]["children"][0]["name"],
        "shot.mov"
    );
}

#[test]
fn clip_with_no_name_or_filename_omits_the_name_field() {
    // `..` has no leaf, so its file_name() is None; with no explicit clip name
    // either, the OTIO `name` is omitted entirely rather than defaulted to a
    // placeholder (OTIO fills its "" default on read).
    let stereo = AudioProperties {
        sample_rate: SampleRate::new(48_000).unwrap(),
        channels: ChannelLayout::Stereo,
    };
    let mut timeline = Timeline::new("demo", FrameRate::whole(30).unwrap());
    timeline
        .add_asset(
            MediaAsset::new(
                MediaSource::file(".."),
                Seconds::from_secs(60),
                None,
                Some(stereo),
            )
            .unwrap(),
        )
        .unwrap();
    let mut track = Track::new(TrackKind::Audio);
    track.push_clip(Clip::new(
        MediaSource::file(".."),
        TimeRange::new(Seconds::ZERO, Seconds::from_secs(2)).unwrap(),
    ));
    timeline.add_track(track);
    timeline.validate().unwrap();

    let value: serde_json::Value = serde_json::from_str(&to_otio(&timeline).unwrap()).unwrap();
    let clip = &value["tracks"]["children"][0]["children"][0];
    assert!(clip.get("name").is_none());
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
