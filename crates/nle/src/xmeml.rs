//! FCP7 `xmeml` (XMEML v5) exporter — the one interchange format that imports
//! natively in **both** Premiere Pro and DaVinci Resolve.
//!
//! This exporter handles multi-track timelines of hard cuts (clips and gaps).
//! A clip's timeline position is the running sum of the clips and gaps before
//! it; xmeml times are integer frames at the sequence timebase. Audio
//! cross-fades (transitions) and fractional NTSC frame rates are tracked as
//! follow-up work — the exporter returns an error rather than emit something an
//! NLE would silently misread.

use std::collections::HashMap;

use hollywood_timeline::{
    Clip, FrameRate, MediaSource, Seconds, Timeline, TimelineError, Track, TrackItem, TrackKind,
};
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

use crate::error::NleError;

type XmlWriter = Writer<Vec<u8>>;

/// Maps each registered asset source to its unique intra-document `file` id.
type AssetIds<'a> = HashMap<&'a MediaSource, String>;

/// Serialize `timeline` to an FCP7 `xmeml` document.
///
/// # Errors
///
/// Returns [`NleError::InvalidTimeline`] if the timeline fails its own
/// validation, [`NleError::UnsupportedFrameRate`] for fractional (NTSC) rates,
/// [`NleError::UnsupportedTransition`] if any track contains a transition, and
/// [`NleError::UnalignedDuration`] if a clip or gap is not a whole number of
/// frames at the sequence rate.
pub fn to_xmeml(timeline: &Timeline) -> Result<String, NleError> {
    // The IR is only coherent once validated; never export an unvalidated one.
    timeline.validate()?;

    let rate = timeline.frame_rate();
    // Fail fast on rates the exporter cannot represent yet.
    rate.as_whole().ok_or(NleError::UnsupportedFrameRate)?;

    let asset_ids: AssetIds<'_> = timeline
        .assets()
        .iter()
        .enumerate()
        .map(|(index, asset)| (asset.source(), format!("file-{}", index + 1)))
        .collect();

    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    write(
        &mut writer,
        Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)),
    )?;

    let mut root = BytesStart::new("xmeml");
    root.push_attribute(("version", "5"));
    write(&mut writer, Event::Start(root))?;

    start(&mut writer, "sequence")?;
    text_element(&mut writer, "name", timeline.name())?;
    write_rate(&mut writer, rate)?;

    start(&mut writer, "media")?;
    write_media_kind(&mut writer, timeline, &asset_ids, rate, TrackKind::Video)?;
    write_media_kind(&mut writer, timeline, &asset_ids, rate, TrackKind::Audio)?;
    end(&mut writer, "media")?;

    end(&mut writer, "sequence")?;
    end(&mut writer, "xmeml")?;

    Ok(String::from_utf8(writer.into_inner())?)
}

fn write_media_kind(
    writer: &mut XmlWriter,
    timeline: &Timeline,
    ids: &AssetIds<'_>,
    rate: FrameRate,
    kind: TrackKind,
) -> Result<(), NleError> {
    let mut tracks = timeline
        .tracks()
        .iter()
        .filter(|t| t.kind() == kind)
        .peekable();
    if tracks.peek().is_none() {
        return Ok(());
    }
    let element = kind_element(kind);
    start(writer, element)?;
    for track in tracks {
        write_track(writer, ids, rate, track)?;
    }
    end(writer, element)
}

/// The xmeml `<media>` child element name for a track kind.
fn kind_element(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Video => "video",
        TrackKind::Audio => "audio",
    }
}

fn write_track(
    writer: &mut XmlWriter,
    ids: &AssetIds<'_>,
    rate: FrameRate,
    track: &Track,
) -> Result<(), NleError> {
    start(writer, "track")?;
    let mut position = Seconds::ZERO;
    for item in track.items() {
        match item {
            TrackItem::Gap(gap) => {
                // Validate every gap, not just those followed by a clip, so a
                // trailing or gap-only track still honours the frame-alignment
                // contract.
                frames_exact(gap.duration(), rate)?;
                position = position
                    .checked_add(gap.duration())
                    .ok_or(NleError::InvalidTimeline(TimelineError::OccupiedOverflow))?;
            }
            TrackItem::Clip(clip) => {
                let start_frame = frames_exact(position, rate)?;
                position = position
                    .checked_add(clip.duration())
                    .ok_or(NleError::InvalidTimeline(TimelineError::OccupiedOverflow))?;
                let end_frame = frames_exact(position, rate)?;
                write_clipitem(writer, ids, clip, rate, start_frame, end_frame)?;
            }
            TrackItem::Transition(_) => return Err(NleError::UnsupportedTransition),
        }
    }
    end(writer, "track")
}

fn write_clipitem(
    writer: &mut XmlWriter,
    ids: &AssetIds<'_>,
    clip: &Clip,
    rate: FrameRate,
    start_frame: i64,
    end_frame: i64,
) -> Result<(), NleError> {
    let source = clip.asset();
    // Unreachable after `validate()` (every clip's asset is registered), but
    // surface it as an error rather than silently emit an undeclared file id.
    let file_id = ids
        .get(source)
        .map(String::as_str)
        .ok_or_else(|| NleError::InvalidTimeline(TimelineError::UnknownAsset(source.clone())))?;
    let name = clip.name().or_else(|| source.file_name()).unwrap_or("clip");
    let in_frame = frames_exact(clip.range().start(), rate)?;
    let out_frame = frames_exact(clip.range().end(), rate)?;

    start(writer, "clipitem")?;
    text_element(writer, "name", name)?;
    write_rate(writer, rate)?;
    text_element(writer, "start", &start_frame.to_string())?;
    text_element(writer, "end", &end_frame.to_string())?;
    text_element(writer, "in", &in_frame.to_string())?;
    text_element(writer, "out", &out_frame.to_string())?;

    let mut file = BytesStart::new("file");
    // `push_attribute((&str, &str))` already escapes the value (quick-xml
    // escapes in `From<(&str, &str)> for Attribute`) — do not escape again.
    file.push_attribute(("id", file_id));
    write(writer, Event::Start(file))?;
    text_element(writer, "name", source.file_name().unwrap_or(file_id))?;
    text_element(writer, "pathurl", &source.to_string())?;
    write(writer, Event::End(BytesEnd::new("file")))?;

    end(writer, "clipitem")
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

fn write_rate(writer: &mut XmlWriter, rate: FrameRate) -> Result<(), NleError> {
    let fps = rate.as_whole().ok_or(NleError::UnsupportedFrameRate)?;
    start(writer, "rate")?;
    text_element(writer, "timebase", &fps.to_string())?;
    text_element(writer, "ntsc", "FALSE")?;
    end(writer, "rate")
}

fn write(writer: &mut XmlWriter, event: Event<'_>) -> Result<(), NleError> {
    Ok(writer.write_event(event)?)
}

fn start(writer: &mut XmlWriter, name: &str) -> Result<(), NleError> {
    write(writer, Event::Start(BytesStart::new(name)))
}

fn end(writer: &mut XmlWriter, name: &str) -> Result<(), NleError> {
    write(writer, Event::End(BytesEnd::new(name)))
}

fn text_element(writer: &mut XmlWriter, name: &str, text: &str) -> Result<(), NleError> {
    start(writer, name)?;
    write(writer, Event::Text(BytesText::new(text)))?;
    end(writer, name)
}
