//! OpenTimelineIO (`.otio`) exporter — an optional interchange path that writes
//! the timeline IR as native OTIO JSON against a pinned schema version.
//!
//! Per ADR 0002 / SPEC §5.4 this is hand-written `serde_json`, not the abandoned
//! OTIO Rust bindings and not a Python runtime: the IR maps directly onto OTIO's
//! object model (a `Timeline` whose `tracks` is a single `Stack` of `Track`s,
//! each holding `Clip`/`Gap`/`Transition` children). Every object carries its
//! pinned `OTIO_SCHEMA` tag. Times are exact and survive without drift:
//! *editorial* times the tool produces (clip in/out, gaps, fades) are emitted as
//! whole frames at the sequence rate, and a sub-frame editorial time is rejected
//! rather than snapped. A clip's `available_range`, by contrast, is the *probed*
//! source length — metadata the tool does not control and which rarely lands on
//! a frame boundary — so it is emitted at the source's own exact rational rate
//! (value = numerator, rate = denominator) and never rejected.
//!
//! Fractional (NTSC) sequence rates are rejected for consistency with the xmeml
//! and FCPXML exporters; unlike those text formats OTIO could represent them
//! natively, so lifting that restriction is deferred follow-up, not an inherent
//! OTIO limit.
//!
//! The output is lean: it omits the OTIO fields the IR has no concept of —
//! `metadata`, `effects`, `markers`, `enabled`, and `global_start_time` — which
//! the upstream reader reads with `read_if_present` and fills with their
//! defaults, so the document still loads cleanly. It does carry the structurally
//! required fields (`tracks`, `children`, `kind`, a transition's offsets and
//! type), each object's `name` where the IR has one (the timeline's name, the
//! top-level stack's conventional `tracks`, and a clip's explicit name or source
//! file name), and an explicit `source_range: null` on stacks and tracks so OTIO
//! derives their range from their children.

use std::collections::HashMap;

use hollywood_timeline::{
    Clip, FrameRate, Gap, MediaAsset, MediaSource, Seconds, Timeline, TimelineError, Track,
    TrackItem, TrackKind, Transition,
};
use serde_json::{Map, Value, json};

use crate::error::NleError;

/// Serialize `timeline` to an OpenTimelineIO (`.otio`) JSON document.
///
/// # Errors
///
/// Returns [`NleError::InvalidTimeline`] if the timeline fails its own
/// validation, [`NleError::UnsupportedFrameRate`] for fractional (NTSC) rates,
/// and [`NleError::UnalignedDuration`] if a clip, gap, or cross-fade is not a
/// whole number of frames at the sequence rate.
pub fn to_otio(timeline: &Timeline) -> Result<String, NleError> {
    // The IR is only coherent once validated; never export an unvalidated one.
    timeline.validate()?;

    let rate = timeline.frame_rate();
    let fps = rate.as_whole().ok_or(NleError::UnsupportedFrameRate)?;
    let assets = asset_index(timeline);

    let tracks = timeline
        .tracks()
        .iter()
        .map(|track| track_value(track, &assets, rate, fps))
        .collect::<Result<Vec<_>, _>>()?;

    let document = json!({
        "OTIO_SCHEMA": "Timeline.1",
        "name": timeline.name(),
        "tracks": {
            "OTIO_SCHEMA": "Stack.1",
            "name": "tracks",
            "source_range": Value::Null,
            "children": tracks,
        },
    });

    Ok(serde_json::to_string_pretty(&document)?)
}

/// Each source mapped to its registered asset, so a clip can resolve the full
/// media duration it needs for an `available_range`.
type AssetIndex<'a> = HashMap<&'a MediaSource, &'a MediaAsset>;

fn asset_index(timeline: &Timeline) -> AssetIndex<'_> {
    timeline
        .assets()
        .iter()
        .map(|asset| (asset.source(), asset))
        .collect()
}

fn track_value(
    track: &Track,
    assets: &AssetIndex<'_>,
    rate: FrameRate,
    fps: u32,
) -> Result<Value, NleError> {
    let children = track
        .items()
        .iter()
        .map(|item| item_value(item, assets, rate, fps))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(json!({
        "OTIO_SCHEMA": "Track.1",
        "kind": track_kind(track.kind()),
        "source_range": Value::Null,
        "children": children,
    }))
}

/// The OTIO `TrackKind` constant for a track's kind.
fn track_kind(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Video => "Video",
        TrackKind::Audio => "Audio",
    }
}

fn item_value(
    item: &TrackItem,
    assets: &AssetIndex<'_>,
    rate: FrameRate,
    fps: u32,
) -> Result<Value, NleError> {
    match item {
        TrackItem::Clip(clip) => clip_value(clip, assets, rate, fps),
        TrackItem::Gap(gap) => gap_value(gap, rate, fps),
        TrackItem::Transition(transition) => transition_value(*transition, rate, fps),
    }
}

fn clip_value(
    clip: &Clip,
    assets: &AssetIndex<'_>,
    rate: FrameRate,
    fps: u32,
) -> Result<Value, NleError> {
    let asset = assets.get(clip.asset()).ok_or_else(|| {
        NleError::InvalidTimeline(TimelineError::UnknownAsset(clip.asset().clone()))
    })?;

    // Build the object directly so `name` can be conditionally present without a
    // post-construction mutation: OTIO reads `name` with `read_if_present`
    // (defaulting to ""), so carry only a name the IR actually has — an explicit
    // clip name, else the source's file name — never an invented placeholder.
    let mut object = Map::new();
    object.insert("OTIO_SCHEMA".to_owned(), Value::from("Clip.1"));
    if let Some(name) = clip.name().or_else(|| clip.asset().file_name()) {
        object.insert("name".to_owned(), Value::from(name));
    }
    object.insert(
        "source_range".to_owned(),
        time_range(clip.range().start(), clip.duration(), rate, fps)?,
    );
    object.insert(
        "media_reference".to_owned(),
        json!({
            "OTIO_SCHEMA": "ExternalReference.1",
            "target_url": clip.asset().to_string(),
            "available_range": available_range(asset.duration()),
        }),
    );
    Ok(Value::Object(object))
}

fn gap_value(gap: &Gap, rate: FrameRate, fps: u32) -> Result<Value, NleError> {
    // A gap is an item whose source range spans its own length from zero.
    Ok(json!({
        "OTIO_SCHEMA": "Gap.1",
        "source_range": time_range(Seconds::ZERO, gap.duration(), rate, fps)?,
    }))
}

fn transition_value(transition: Transition, rate: FrameRate, fps: u32) -> Result<Value, NleError> {
    // OTIO places the transition between its two clips and reaches `in_offset`
    // into the preceding clip and `out_offset` into the following one. Mirror the
    // xmeml centering convention: `lead` frames before the cut, the rest after.
    let span = frames_exact(transition.duration(), rate)?;
    let lead = span / 2;
    Ok(json!({
        "OTIO_SCHEMA": "Transition.1",
        "transition_type": "SMPTE_Dissolve",
        "in_offset": rational_time(lead, fps),
        "out_offset": rational_time(span - lead, fps),
    }))
}

/// An OTIO `TimeRange` of `duration` starting at `start`, both expressed as whole
/// frames at the sequence rate.
fn time_range(
    start: Seconds,
    duration: Seconds,
    rate: FrameRate,
    fps: u32,
) -> Result<Value, NleError> {
    Ok(json!({
        "OTIO_SCHEMA": "TimeRange.1",
        "start_time": rational_time(frames_exact(start, rate)?, fps),
        "duration": rational_time(frames_exact(duration, rate)?, fps),
    }))
}

/// An OTIO `RationalTime` of `frames` at `fps`.
fn rational_time(frames: i64, fps: u32) -> Value {
    json!({
        "OTIO_SCHEMA": "RationalTime.1",
        "rate": fps,
        "value": frames,
    })
}

/// The media's `available_range`: its full length from zero, expressed exactly at
/// the source's own rational rate rather than the timeline frame grid. Source
/// length is probed metadata the tool does not control (48 kHz audio is rarely a
/// whole number of frames, and container durations seldom are), so it must never
/// be rejected for not landing on a frame boundary.
fn available_range(duration: Seconds) -> Value {
    json!({
        "OTIO_SCHEMA": "TimeRange.1",
        "start_time": rational_time_exact(Seconds::ZERO),
        "duration": rational_time_exact(duration),
    })
}

/// An OTIO `RationalTime` carrying `seconds` exactly: value = numerator, rate =
/// denominator, so any rational duration round-trips without frame snapping.
fn rational_time_exact(seconds: Seconds) -> Value {
    let (value, rate) = seconds.as_exact_rational();
    json!({
        "OTIO_SCHEMA": "RationalTime.1",
        "rate": rate,
        "value": value,
    })
}

/// Convert `seconds` to whole frames at `rate`, erroring if it is not exactly a
/// frame boundary — the exporter never silently snaps a misaligned time.
fn frames_exact(seconds: Seconds, rate: FrameRate) -> Result<i64, NleError> {
    let frames = seconds.to_frames(rate);
    if Seconds::from_frames(frames, rate) == seconds {
        Ok(frames)
    } else {
        Err(NleError::UnalignedDuration)
    }
}
