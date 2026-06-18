# 0007 — Auto-framing emits NLE-native keyframed transforms, not baked video

## Status

Proposed.

## Context

Once clips are assembled into one scene, Hollywood will add camera-style motion
the editor would otherwise keyframe by hand (§5.9): a content-aware zoom onto
the region of a frame that actually changes, and a slow **Ken Burns** zoom
across long, low-activity clips.

A zoom is a per-clip crop/scale/position over time. It can be delivered either
as **baked pixels** (re-rendered, cropped/scaled video) or as an **NLE-native
keyframed transform** the editor can adjust or remove.

Unlike audio filters, basic transform/crop keyframes interchange comparatively
well: FCPXML has `adjust-transform` and `adjust-crop` with keyframes, and FCP7
xmeml has basic-motion (scale/center) keyframes. Re-rendering video would also
make Hollywood a video renderer, which §2 explicitly rules out, and would force
codec/quality/colorspace decisions that belong in the NLE.

## Decision

Auto-framing is expressed as a **transform/crop with keyframes** in the IR
(`hollywood-timeline`) and exported as **native NLE motion** by `hollywood-nle`.
Hollywood never renders video pixels. Because transform interchange still varies
by NLE, it is covered by the golden-file corpus and validated against real
imports, with a **static-transform fallback** where a target NLE mishandles
keyframes.

## Consequences

- The editor can refine or delete every move; Hollywood stays a non-renderer
  (§2), avoiding all video codec/quality concerns.
- Auto-framing depends on a new **video frame decode** capability on the
  `hollywood-ffmpeg` trait (§5.5) — downscaled frames are enough for analysis.
- Keyframed-transform export joins cross-fades as a **higher-risk NLE
  deliverable** (ADR 0002): per-NLE quirks must be pinned by golden files before
  it ships.
- Analysis runs on decoded frames only; no rendering pipeline or GPU effect
  graph is introduced.

## Alternatives considered

- **Baked/re-rendered video** — rejected: makes Hollywood a renderer (§2),
  forces codec/quality/colorspace choices, and removes editor control of the
  move.
- **No motion (static crop only)** — kept as the fallback, but a fixed crop
  wastes the long, static clips Ken Burns is meant to enliven.
