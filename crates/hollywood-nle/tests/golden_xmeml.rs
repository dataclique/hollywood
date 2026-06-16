//! Golden-file tests for the xmeml exporter. The checked-in golden under
//! `tests/golden/` is the compatibility contract; a mismatch is a regression.
//! Delete a golden and re-run to regenerate it after an intentional change.

use std::path::PathBuf;

use hollywood_nle::to_xmeml;
use hollywood_timeline::{
    Clip, FrameRate, Gap, MediaAsset, MediaSource, Seconds, TimeRange, Timeline, Track, TrackKind,
    Transition,
};

#[test]
fn two_clip_video_track_matches_golden() {
    let mut timeline = Timeline::new("demo", FrameRate::whole(30).unwrap());
    for id in ["a.mov", "b.mov"] {
        timeline
            .add_asset(
                MediaAsset::new(MediaSource::file(id), Seconds::from_secs(60), None, None).unwrap(),
            )
            .unwrap();
    }

    let mut track = Track::new(TrackKind::Video);
    track.push_clip(Clip::with_name(
        MediaSource::file("a.mov"),
        TimeRange::new(Seconds::from_secs(2), Seconds::from_secs(3)).unwrap(),
        "intro",
    ));
    track.push_gap(Gap::new(Seconds::from_secs(1)).unwrap());
    track.push_clip(Clip::with_name(
        MediaSource::file("b.mov"),
        TimeRange::new(Seconds::from_secs(10), Seconds::from_secs(4)).unwrap(),
        "outro",
    ));
    timeline.add_track(track);
    timeline.validate().unwrap();

    assert_golden("two_clip_video_track.xml", &to_xmeml(&timeline).unwrap()).unwrap();
}

#[test]
fn transitions_are_rejected_for_now() {
    let mut timeline = Timeline::new("demo", FrameRate::whole(30).unwrap());
    timeline
        .add_asset(
            MediaAsset::new(
                MediaSource::file("a.mov"),
                Seconds::from_secs(60),
                None,
                None,
            )
            .unwrap(),
        )
        .unwrap();

    let mut track = Track::new(TrackKind::Video);
    track.push_clip(Clip::new(
        MediaSource::file("a.mov"),
        TimeRange::new(Seconds::ZERO, Seconds::from_secs(2)).unwrap(),
    ));
    track
        .push_transition(Transition::cross_fade(Seconds::from_secs(1)).unwrap())
        .unwrap();
    track.push_clip(Clip::new(
        MediaSource::file("a.mov"),
        TimeRange::new(Seconds::from_secs(3), Seconds::from_secs(2)).unwrap(),
    ));
    timeline.add_track(track);

    assert!(to_xmeml(&timeline).is_err());
}

/// Compare `actual` against the checked-in golden, or write it if absent (first
/// run bootstraps the file to be committed).
fn assert_golden(name: &str, actual: &str) -> std::io::Result<()> {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "golden", name]
        .iter()
        .collect();
    if let Ok(expected) = std::fs::read_to_string(&path) {
        assert_eq!(actual, expected, "golden mismatch for {name}");
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, actual)?;
    }
    Ok(())
}
