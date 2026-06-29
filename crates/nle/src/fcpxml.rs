//! FCPXML exporter — the interchange format Final Cut Pro and DaVinci Resolve
//! prefer over legacy FCP7 `xmeml`.
//!
//! FCPXML separates *resources* (each source file is an `<asset>`, each pixel
//! geometry + frame rate an `<format>`) from the *timeline* (a `<sequence>`
//! whose `<spine>` lays the primary track end to end, with every other track
//! hung off the first spine element as a connected clip on its own `lane`).
//! Times are exact rational seconds (`<frames>/<fps>s`), so the IR's rational
//! time survives serialization without drift.
//!
//! Scope matches the `xmeml` exporter: multi-track hard cuts only. Audio
//! cross-fades (transitions), fractional NTSC rates, and source durations that
//! are not a whole number of frames are rejected rather than silently
//! misrepresented — each is tracked follow-up work.
//!
//! Like the `xmeml` goldens, the checked-in FCPXML goldens pin this exporter's
//! output against regressions; they are not yet validated against a real Final
//! Cut / Resolve import.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use hollywood_timeline::{
    Clip, FrameRate, MediaAsset, MediaSource, Seconds, Timeline, TimelineError, Track, TrackItem,
    TrackKind,
};
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

use crate::error::NleError;

type XmlWriter = Writer<Vec<u8>>;

/// FCPXML document version this exporter targets. Pinned per SPEC; the exact
/// version range that Final Cut Pro and DaVinci Resolve both accept on import is
/// not yet validated against a real NLE (tracked follow-up on #37).
const FCPXML_VERSION: &str = "1.10";

/// Default sequence geometry when the timeline carries no video asset to take
/// it from — an audio-only timeline still needs a `<format>` for its sequence.
const DEFAULT_WIDTH: u32 = 1920;
const DEFAULT_HEIGHT: u32 = 1080;

/// The sequence format is interned first in [`Resources::build`], so it is
/// always the first resource id.
const SEQUENCE_FORMAT_REF: &str = "r1";

/// Serialize `timeline` to an FCPXML document.
///
/// # Errors
///
/// Returns [`NleError::InvalidTimeline`] if the timeline fails its own
/// validation, [`NleError::UnsupportedFrameRate`] for fractional (NTSC) rates,
/// [`NleError::UnsupportedTransition`] if any track contains a transition, and
/// [`NleError::UnalignedDuration`] if a clip, gap, or source duration is not a
/// whole number of frames at the sequence rate.
pub fn to_fcpxml(timeline: &Timeline) -> Result<String, NleError> {
    // The IR is only coherent once validated; never export an unvalidated one.
    timeline.validate()?;

    let rate = timeline.frame_rate();
    let fps = rate.as_whole().ok_or(NleError::UnsupportedFrameRate)?;
    let resources = Resources::build(timeline, fps)?;

    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    write(
        &mut writer,
        Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)),
    )?;
    write(&mut writer, Event::DocType(BytesText::new("fcpxml")))?;

    let mut root = BytesStart::new("fcpxml");
    root.push_attribute(("version", FCPXML_VERSION));
    write(&mut writer, Event::Start(root))?;

    write_resources(&mut writer, &resources, rate)?;
    write_library(&mut writer, &resources, timeline, rate)?;

    end(&mut writer, "fcpxml")?;
    Ok(String::from_utf8(writer.into_inner())?)
}

/// A video `<format>` resource: pixel geometry at a whole frame rate.
struct Format {
    fps: u32,
    width: u32,
    height: u32,
}

/// The FCPXML `<resources>` model: deduplicated `<format>`s followed by one
/// `<asset>` per source. Resource ids are positional — formats are `r1..`, then
/// assets continue the numbering — so an id resolves without a stored counter.
struct Resources<'a> {
    formats: Vec<Format>,
    format_index: HashMap<(u32, u32, u32), usize>,
    assets: Vec<&'a MediaAsset>,
    asset_index: HashMap<&'a MediaSource, usize>,
}

impl<'a> Resources<'a> {
    fn build(timeline: &'a Timeline, seq_fps: u32) -> Result<Self, NleError> {
        let mut formats = Vec::new();
        let mut format_index = HashMap::new();

        // The sequence format is interned first, so it is always `r1`. Its
        // geometry comes from the first video asset, or a sensible default for
        // an audio-only timeline.
        let (width, height) = timeline
            .assets()
            .iter()
            .find_map(|asset| asset.video().map(|video| (video.width, video.height)))
            .unwrap_or((DEFAULT_WIDTH, DEFAULT_HEIGHT));
        intern_format(&mut formats, &mut format_index, (seq_fps, width, height));

        let mut asset_index = HashMap::new();
        let mut assets = Vec::new();
        for asset in timeline.assets() {
            if let Some(video) = asset.video() {
                let fps = video
                    .frame_rate
                    .as_whole()
                    .ok_or(NleError::UnsupportedFrameRate)?;
                intern_format(
                    &mut formats,
                    &mut format_index,
                    (fps, video.width, video.height),
                );
            }
            asset_index.insert(asset.source(), assets.len());
            assets.push(asset);
        }

        Ok(Self {
            formats,
            format_index,
            assets,
            asset_index,
        })
    }

    /// The resource id for a format geometry, which must have been interned.
    fn format_ref(&self, key: (u32, u32, u32)) -> Option<String> {
        self.format_index
            .get(&key)
            .map(|index| format!("r{}", index + 1))
    }

    /// The resource id for a registered asset source.
    fn asset_ref(&self, source: &MediaSource) -> Option<String> {
        self.asset_index
            .get(source)
            .map(|index| format!("r{}", self.formats.len() + index + 1))
    }
}

fn intern_format(
    formats: &mut Vec<Format>,
    index: &mut HashMap<(u32, u32, u32), usize>,
    key: (u32, u32, u32),
) {
    if let Entry::Vacant(entry) = index.entry(key) {
        entry.insert(formats.len());
        formats.push(Format {
            fps: key.0,
            width: key.1,
            height: key.2,
        });
    }
}

fn write_resources(
    writer: &mut XmlWriter,
    resources: &Resources<'_>,
    rate: FrameRate,
) -> Result<(), NleError> {
    start(writer, "resources")?;
    for (index, format) in resources.formats.iter().enumerate() {
        empty_element(
            writer,
            "format",
            &[
                ("id", format!("r{}", index + 1)),
                ("frameDuration", format!("1/{}s", format.fps)),
                ("width", format.width.to_string()),
                ("height", format.height.to_string()),
            ],
        )?;
    }
    for (index, asset) in resources.assets.iter().enumerate() {
        write_asset(
            writer,
            resources,
            rate,
            asset,
            resources.formats.len() + index + 1,
        )?;
    }
    end(writer, "resources")
}

fn write_asset(
    writer: &mut XmlWriter,
    resources: &Resources<'_>,
    rate: FrameRate,
    asset: &MediaAsset,
    id_number: usize,
) -> Result<(), NleError> {
    let id = format!("r{id_number}");
    let name = asset.source().file_name().unwrap_or(&id).to_string();
    let has_video = asset.video().is_some();
    let has_audio = asset.audio().is_some();

    let mut attrs = vec![
        ("id", id),
        ("name", name),
        ("start", "0s".to_string()),
        ("duration", fcp_time(asset.duration(), rate)?),
        ("hasVideo", bool_attr(has_video)),
        ("hasAudio", bool_attr(has_audio)),
    ];
    if let Some(video) = asset.video() {
        // Whole-ness was already established in `Resources::build`.
        let fps = video
            .frame_rate
            .as_whole()
            .ok_or(NleError::UnsupportedFrameRate)?;
        let format_ref = resources
            .format_ref((fps, video.width, video.height))
            .ok_or(NleError::UnsupportedFrameRate)?;
        attrs.push(("format", format_ref));
    }
    if let Some(audio) = asset.audio() {
        attrs.push(("audioSources", "1".to_string()));
        attrs.push(("audioChannels", audio.channels.count().to_string()));
        attrs.push(("audioRate", audio.sample_rate.hertz().to_string()));
    }

    start_element(writer, "asset", &attrs)?;
    empty_element(
        writer,
        "media-rep",
        &[
            ("kind", "original-media".to_string()),
            ("src", asset.source().to_string()),
        ],
    )?;
    end(writer, "asset")
}

fn write_library(
    writer: &mut XmlWriter,
    resources: &Resources<'_>,
    timeline: &Timeline,
    rate: FrameRate,
) -> Result<(), NleError> {
    start(writer, "library")?;
    start_element(writer, "event", &[("name", timeline.name().to_string())])?;
    start_element(writer, "project", &[("name", timeline.name().to_string())])?;

    let total = total_duration(timeline)?;
    start_element(
        writer,
        "sequence",
        &[
            ("format", SEQUENCE_FORMAT_REF.to_string()),
            ("duration", fcp_time(total, rate)?),
            ("tcStart", "0s".to_string()),
            ("tcFormat", "NDF".to_string()),
        ],
    )?;
    write_spine(writer, resources, timeline, rate)?;
    end(writer, "sequence")?;

    end(writer, "project")?;
    end(writer, "event")?;
    end(writer, "library")
}

/// The longest track decides the sequence length; a transition does not advance
/// a track, so [`Track::occupied`] already excludes it.
fn total_duration(timeline: &Timeline) -> Result<Seconds, NleError> {
    timeline
        .tracks()
        .iter()
        .try_fold(Seconds::ZERO, |max, track| Ok(max.max(track.occupied()?)))
}

fn write_spine(
    writer: &mut XmlWriter,
    resources: &Resources<'_>,
    timeline: &Timeline,
    rate: FrameRate,
) -> Result<(), NleError> {
    start(writer, "spine")?;

    let tracks = timeline.tracks();
    let Some((primary_index, primary)) = primary_track(tracks) else {
        // No items on any track: an empty spine is still a valid sequence.
        return end(writer, "spine");
    };

    // Every non-primary track is emitted as connected clips nested in the first
    // spine element. A connected clip's `offset` is in its host's LOCAL time,
    // anchored at the host's source in-point — so it is the host's `start` plus
    // the clip's absolute timeline offset (the host sits at sequence offset 0).
    // A gap host has an implicit `start` of zero; a clip host carries its
    // in-point, which is non-zero whenever footage was trimmed. Video lanes
    // composite above the primary, audio lanes below.
    let connected = collect_connected(resources, rate, tracks, primary_index)?;

    let mut position = Seconds::ZERO;
    for (item_index, item) in primary.items().iter().enumerate() {
        let host = item_index == 0 && !connected.is_empty();
        match item {
            TrackItem::Gap(gap) => {
                let attrs = [
                    ("offset", fcp_time(position, rate)?),
                    ("duration", fcp_time(gap.duration(), rate)?),
                ];
                if host {
                    start_element(writer, "gap", &attrs)?;
                    write_connected(writer, resources, rate, &connected, Seconds::ZERO)?;
                    end(writer, "gap")?;
                } else {
                    empty_element(writer, "gap", &attrs)?;
                }
                position = advance(position, gap.duration())?;
            }
            TrackItem::Clip(clip) => {
                let attrs = asset_clip_attrs(resources, rate, clip, position, None)?;
                if host {
                    start_element(writer, "asset-clip", &attrs)?;
                    write_connected(writer, resources, rate, &connected, clip.range().start())?;
                    end(writer, "asset-clip")?;
                } else {
                    empty_element(writer, "asset-clip", &attrs)?;
                }
                position = advance(position, clip.duration())?;
            }
            TrackItem::Transition(_) => return Err(NleError::UnsupportedTransition),
        }
    }

    end(writer, "spine")
}

/// The track that drives the FCPXML spine: the first non-empty **video** track,
/// else the first non-empty audio track. FCPXML collapses the timeline onto one
/// primary storyline (everything else hangs off it on connected lanes), so a
/// later video track must still win the spine over an earlier audio track to
/// keep the A-roll/B-roll hierarchy that the multi-track IR expresses.
fn primary_track(tracks: &[Track]) -> Option<(usize, &Track)> {
    let first_non_empty = |kind: TrackKind| {
        tracks
            .iter()
            .enumerate()
            .find(move |(_, track)| track.kind() == kind && !track.items().is_empty())
    };
    first_non_empty(TrackKind::Video).or_else(|| first_non_empty(TrackKind::Audio))
}

/// A clip from a non-primary track, placed at its absolute timeline `offset` on
/// a composite `lane`.
struct Connected<'a> {
    clip: &'a Clip,
    lane: i32,
    offset: Seconds,
}

fn collect_connected<'a>(
    resources: &Resources<'_>,
    rate: FrameRate,
    tracks: &'a [Track],
    primary_index: usize,
) -> Result<Vec<Connected<'a>>, NleError> {
    let mut connected = Vec::new();
    let mut next_video_lane = 1;
    let mut next_audio_lane = -1;
    for (index, track) in tracks.iter().enumerate() {
        if index == primary_index || track.items().is_empty() {
            continue;
        }
        let lane = match track.kind() {
            TrackKind::Video => take_lane(&mut next_video_lane, 1),
            TrackKind::Audio => take_lane(&mut next_audio_lane, -1),
        };
        let mut position = Seconds::ZERO;
        for item in track.items() {
            match item {
                TrackItem::Gap(gap) => {
                    // A gap is implicit between connected clips (each carries an
                    // absolute offset), but still must be frame-aligned.
                    frames_exact(gap.duration(), rate)?;
                    position = advance(position, gap.duration())?;
                }
                TrackItem::Clip(clip) => {
                    // Resolve the ref now so an unknown asset fails before emit.
                    resources
                        .asset_ref(clip.asset())
                        .ok_or_else(|| unknown_asset(clip))?;
                    connected.push(Connected {
                        clip,
                        lane,
                        offset: position,
                    });
                    position = advance(position, clip.duration())?;
                }
                TrackItem::Transition(_) => return Err(NleError::UnsupportedTransition),
            }
        }
    }
    Ok(connected)
}

fn write_connected(
    writer: &mut XmlWriter,
    resources: &Resources<'_>,
    rate: FrameRate,
    connected: &[Connected<'_>],
    host_start: Seconds,
) -> Result<(), NleError> {
    for item in connected {
        // Express the connected clip's offset in the host's local time: the
        // host's in-point plus the clip's absolute timeline offset. The importer
        // resolves it back to `host.offset(0) + (offset - host.start)` = the
        // intended sequence time.
        let offset = advance(host_start, item.offset)?;
        let attrs = asset_clip_attrs(resources, rate, item.clip, offset, Some(item.lane))?;
        empty_element(writer, "asset-clip", &attrs)?;
    }
    Ok(())
}

fn asset_clip_attrs(
    resources: &Resources<'_>,
    rate: FrameRate,
    clip: &Clip,
    offset: Seconds,
    lane: Option<i32>,
) -> Result<Vec<(&'static str, String)>, NleError> {
    let asset_ref = resources
        .asset_ref(clip.asset())
        .ok_or_else(|| unknown_asset(clip))?;
    let name = clip
        .name()
        .or_else(|| clip.asset().file_name())
        .unwrap_or("clip")
        .to_string();

    let mut attrs = vec![("ref", asset_ref)];
    if let Some(lane) = lane {
        attrs.push(("lane", lane.to_string()));
    }
    attrs.push(("offset", fcp_time(offset, rate)?));
    attrs.push(("name", name));
    attrs.push(("start", fcp_time(clip.range().start(), rate)?));
    attrs.push(("duration", fcp_time(clip.duration(), rate)?));
    Ok(attrs)
}

/// `position + span`, mapping overflow to the IR's own error so the exporter
/// never panics on an out-of-range timeline.
fn advance(position: Seconds, span: Seconds) -> Result<Seconds, NleError> {
    position
        .checked_add(span)
        .ok_or(NleError::InvalidTimeline(TimelineError::OccupiedOverflow))
}

fn take_lane(next: &mut i32, step: i32) -> i32 {
    let lane = *next;
    *next += step;
    lane
}

fn unknown_asset(clip: &Clip) -> NleError {
    NleError::InvalidTimeline(TimelineError::UnknownAsset(clip.asset().clone()))
}

fn bool_attr(value: bool) -> String {
    if value { "1" } else { "0" }.to_string()
}

/// Format `seconds` as an FCPXML rational time at `rate`, e.g. `90/30s`, erroring
/// if it is not exactly a frame boundary — the exporter never silently snaps.
fn fcp_time(seconds: Seconds, rate: FrameRate) -> Result<String, NleError> {
    let frames = frames_exact(seconds, rate)?;
    if frames == 0 {
        return Ok("0s".to_string());
    }
    let fps = rate.as_whole().ok_or(NleError::UnsupportedFrameRate)?;
    Ok(format!("{frames}/{fps}s"))
}

/// Convert `seconds` to whole frames at `rate`, erroring if it is not exactly a
/// frame boundary.
fn frames_exact(seconds: Seconds, rate: FrameRate) -> Result<i64, NleError> {
    let frames = seconds.to_frames(rate);
    if Seconds::from_frames(frames, rate) == seconds {
        Ok(frames)
    } else {
        Err(NleError::UnalignedDuration)
    }
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

fn start_element(
    writer: &mut XmlWriter,
    name: &str,
    attrs: &[(&str, String)],
) -> Result<(), NleError> {
    write(writer, Event::Start(element(name, attrs)))
}

fn empty_element(
    writer: &mut XmlWriter,
    name: &str,
    attrs: &[(&str, String)],
) -> Result<(), NleError> {
    write(writer, Event::Empty(element(name, attrs)))
}

fn element<'a>(name: &'a str, attrs: &[(&str, String)]) -> BytesStart<'a> {
    // `with_attributes` pushes each `(&str, &str)` through `push_attribute`, which
    // escapes the value itself.
    BytesStart::new(name).with_attributes(attrs.iter().map(|(key, value)| (*key, value.as_str())))
}
