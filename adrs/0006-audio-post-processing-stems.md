# 0006 — Audio post-processing renders stems, not NLE-native filters

## Status

Proposed.

## Context

After the cut is assembled, Hollywood will condition the audio so the rough
assembly is comfortable to listen to without manual gain-riding: per-clip
loudness normalization, corrective auto-EQ, per-track compression/limiting to a
target **LUFS**, and **sidechain ducking** of music under speech (§5.8).

Each of these can in principle be expressed two ways in the exported timeline:

1. As **NLE-native audio filters** (gain/EQ/compressor parameters) attached to
   clips, for the editor to tweak.
2. As **rendered audio stems** — new audio files with the processing baked in,
   referenced by the timeline as ordinary clips.

NLE audio-filter interchange is even less portable than cross-fades: filter
names, parameter ranges, and units differ across Premiere, Resolve, and Final
Cut, and most do not survive xmeml/FCPXML round-trips at all. The project's core
guarantee is that the exported timeline opens cleanly in every target NLE (§2,
ADR 0002).

## Decision

The IR models the **audio-effect chain** as intent (in `hollywood-timeline`),
and the `hollywood-audio` stage **renders that chain to audio stem files**; the
export references the stems as plain clips. Original media stays relinkable, and
the whole audio stage is opt-in, so the editor can revert to untouched audio.

Rendering corrective **audio** stems is the single, deliberate exception to the
"Hollywood assembles, the NLE finishes" rule (§2). It never renders video.

## Consequences

- The exported timeline opens with already-conditioned audio in any NLE, with no
  fragile filter interchange.
- The processing parameters are **baked**: the editor adjusts by relinking to
  originals and re-running, not by editing filter knobs in the NLE. Acceptable
  because the processing is corrective, not creative.
- **Rendering granularity is an open implementation question** for the relevant
  issue: per-clip stems preserve per-clip editing but cannot express track-level
  dynamics/sidechain; per-track stems express them but collapse a track to one
  rendered clip. Likely a mix (clip-level normalize/EQ, track-level
  dynamics/duck) — decided when the stage is built, not here.
- Adds an audio encode step and stem-file management to the pipeline and its
  reproducibility/caching story.

## Alternatives considered

- **NLE-native audio filters** — rejected: unreliable across NLEs and
  round-trips, defeating the open-everywhere guarantee.
- **Single pre-mixed master** — rejected: destroys multi-track editing entirely.
