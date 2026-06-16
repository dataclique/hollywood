//! FCP7 `xmeml` (XMEML v5) exporter — the one interchange format that imports
//! natively in **both** Premiere Pro and DaVinci Resolve.
//!
//! This exporter handles multi-track timelines of hard cuts (clips and gaps).
//! A clip's timeline position is the running sum of the clips and gaps before
//! it; xmeml times are integer frames at the sequence timebase. Audio
//! cross-fades (transitions) and fractional NTSC frame rates are tracked as
//! follow-up work — the exporter returns an error rather than emit something an
//! NLE would silently misread.

use hollywood_timeline::{Clip, FrameRate, Seconds, Timeline, Track, TrackItem, TrackKind};
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

use crate::error::NleError;

type XmlWriter = Writer<Vec<u8>>;

/// Serialize `timeline` to an FCP7 `xmeml` document.
///
/// # Errors
///
/// Returns [`NleError::UnsupportedFrameRate`] for fractional (NTSC) rates and
/// [`NleError::UnsupportedTransition`] if any track contains a transition.
pub fn to_xmeml(timeline: &Timeline) -> Result<String, NleError> {
    let rate = timeline.frame_rate();
    // Fail fast on rates the exporter cannot represent yet.
    rate.as_whole().ok_or(NleError::UnsupportedFrameRate)?;

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
    write_media_kind(&mut writer, timeline, rate, TrackKind::Video, "video")?;
    write_media_kind(&mut writer, timeline, rate, TrackKind::Audio, "audio")?;
    end(&mut writer, "media")?;

    end(&mut writer, "sequence")?;
    end(&mut writer, "xmeml")?;

    String::from_utf8(writer.into_inner()).map_err(|e| NleError::Encoding(e.to_string()))
}

fn write_media_kind(
    writer: &mut XmlWriter,
    timeline: &Timeline,
    rate: FrameRate,
    kind: TrackKind,
    element: &str,
) -> Result<(), NleError> {
    let tracks: Vec<&Track> = timeline
        .tracks()
        .iter()
        .filter(|t| t.kind() == kind)
        .collect();
    if tracks.is_empty() {
        return Ok(());
    }
    start(writer, element)?;
    for track in tracks {
        write_track(writer, rate, track)?;
    }
    end(writer, element)
}

fn write_track(writer: &mut XmlWriter, rate: FrameRate, track: &Track) -> Result<(), NleError> {
    start(writer, "track")?;
    let mut position = Seconds::ZERO;
    for item in track.items() {
        match item {
            TrackItem::Gap(gap) => position += gap.duration(),
            TrackItem::Clip(clip) => {
                let start_frame = position.to_frames(rate);
                position += clip.duration();
                let end_frame = position.to_frames(rate);
                write_clipitem(writer, clip, rate, start_frame, end_frame)?;
            }
            TrackItem::Transition(_) => return Err(NleError::UnsupportedTransition),
        }
    }
    end(writer, "track")
}

fn write_clipitem(
    writer: &mut XmlWriter,
    clip: &Clip,
    rate: FrameRate,
    start_frame: i64,
    end_frame: i64,
) -> Result<(), NleError> {
    start(writer, "clipitem")?;
    let name = clip.name().unwrap_or_else(|| clip.asset().as_str());
    text_element(writer, "name", name)?;
    write_rate(writer, rate)?;
    text_element(writer, "start", &start_frame.to_string())?;
    text_element(writer, "end", &end_frame.to_string())?;
    text_element(
        writer,
        "in",
        &clip.source().start().to_frames(rate).to_string(),
    )?;
    text_element(
        writer,
        "out",
        &clip.source().end().to_frames(rate).to_string(),
    )?;

    let mut file = BytesStart::new("file");
    file.push_attribute(("id", clip.asset().as_str()));
    write(writer, Event::Start(file))?;
    text_element(writer, "name", clip.asset().as_str())?;
    write(writer, Event::End(BytesEnd::new("file")))?;

    end(writer, "clipitem")
}

fn write_rate(writer: &mut XmlWriter, rate: FrameRate) -> Result<(), NleError> {
    let fps = rate.as_whole().ok_or(NleError::UnsupportedFrameRate)?;
    start(writer, "rate")?;
    text_element(writer, "timebase", &fps.to_string())?;
    text_element(writer, "ntsc", "FALSE")?;
    end(writer, "rate")
}

fn write(writer: &mut XmlWriter, event: Event<'_>) -> Result<(), NleError> {
    writer
        .write_event(event)
        .map_err(|e| NleError::Xml(e.to_string()))
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
