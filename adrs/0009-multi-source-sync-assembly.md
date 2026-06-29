# 0009 — Multi-source ingest, cross-source sync, and multi-track assembly

## Status

Proposed.

## Context

Hollywood's purpose includes synchronizing audio from **multiple sources** (a
camera's on-board mic, one or more external recorders) before trimming dead air.
The whole reason `hollywood-sync` exists — cross-correlation `align`, GCC-PHAT,
the drift map — is to place clips from several sources on one timebase.

But the implemented `flow` (ADR 0008) is single-source:

- `Decoded` holds **one** asset + sample buffer;
- `sync` is a no-op pass-through (its own comment: "cross-source alignment …
  lands with multi-source input");
- `assemble(name, rate, asset, &[regions])` lays **one** source onto **one**
  track.

ADR 0008 fixed the typed-state _shape_
(`Sources → … → Synced → Assembled →
Exported`) and said multi-source is
"modeled in the state types from the start"; the first implementation took a
single-source shortcut and deferred the semantics ("the assemble fan-in … three
with multi-source, once multi-source lands"). This ADR settles those semantics.
It decides _what multi-source means_, not the wiring shape (0008) or the
algorithms (`align`/`drift_map` already exist).

The open questions:

1. **Entry state** — how many sources, and is one of them special?
2. **The cut** — across N sources, what decides which moments are kept?
3. **Stage order** — does `Detect → Sync → Assemble` survive, or must sync run
   before detection?
4. **Assembly** — how do per-source offsets become one coherent multi-track
   timeline?
5. **Drift** — where does `drift_map` participate?

The cut question (2) is the product-defining one. The common multi-source job is
an interview or podcast: two or more mics, speech **alternating** between
speakers. A cut driven by a single "reference" mic would gate the _other_
speaker's audio as silence and delete the cross-talk — it would cut conversation
in half. So the cut must consider every source.

## Decision

Thread N sources through the existing `Sources → … → Exported` states, with
these semantics:

### Sources carry a non-empty set with a sync anchor

`Decoded` becomes a non-empty collection of per-source decoded entries (each:
the probed `MediaAsset`, its mono `&[f32]` samples, its `SampleRate`), realizing
ADR 0008's "`Decoded` holds the per-source samples that both `detect` and `sync`
read". One source is the **sync anchor** — the timebase every offset is measured
against — chosen explicitly by the caller (no hidden default; the anchor is a
config input, defaulting to nothing the caller did not order). Single-source is
the degenerate N = 1 case: the anchor is the only source, sync is empty, and the
flow runs exactly as today.

The anchor is a **sync** concept only — the clock the timeline runs on. It is
**not** the cut driver (see below). The anchor's own track is typically the
video (the camera), so the visual timebase is stable.

### The cut is the union of every source's speech

`detect` runs **per source** on that source's own samples, producing per-source
keep regions (`Detected` carries the regions for each source). `assemble` then
fans in (per-source assets, per-source regions, per-source offsets) and forms
the cut as the **union** of all sources' keep regions on the anchor timebase: a
moment survives the cut if **any** source has speech there. This keeps both
speakers in an interview, and reduces to the single-source behavior when N = 1.

Concretely, in the `Assembled` transform: shift each source's regions onto the
anchor timebase by that source's offset, union the shifted regions into one cut,
conform the cut to whole frames (the existing `conform_to_frames`), then lay
each source onto its own track — every track cut at the **same** union regions,
each clip taken from `(union region − that source's offset)` in that source's
media, so the tracks stay locked together. A union region a given source does
not cover (its offset pushes it past the source's bounds) becomes a gap on that
track, preserving alignment.

This preserves `PipelineStage::ORDER` (`Detect → Sync → Assemble`): detection
reads only each source's own samples, so it does not need sync first; the
cross-source combination lives **inside** the assemble fan-in, exactly where ADR
0008 put it.

### Sync aligns every non-anchor source to the anchor

`sync` stops being a no-op: for each non-anchor source it calls
`align(anchor_samples, source_samples, method)` → a `SyncOffset`, and gains a
real `SyncError` path (as its current comment anticipates). `Synced` carries
each source paired with its offset (the anchor's offset is zero). The
`CorrelationMethod` is config (ADR 0008), not a default.

### Drift is a refinement on offset application, not on the threading

A single `SyncOffset` assumes both clocks tick alike. For a long take, a
per-source `DriftMap` (already available via `drift_map`) replaces the constant
offset with an offset-over-time, applied when shifting that source's clips.
Drift changes _clip-level geometry_, not the state types or stage order, so it
is a follow-up that slots into the same `Synced` state (offset _or_ drift map
per source) without reshaping the flow.

### Implementation order

The flow already threads single-source. This ADR lands as stacked PRs, each
independently buildable:

1. per-source state types + the anchor + the N-source probe/decode front;
2. the real `sync` stage (align each source to the anchor);
3. union-cut multi-track assembly (extend `hollywood-assemble`; single-source
   stays the N = 1 path);
4. CLI/GUI multiple-input surface.

This ADR fixes the semantics only; the PRs follow once accepted.

## Consequences

- The cut is correct for the central interview/podcast case — alternating speech
  is kept, not halved — and identical to today for N = 1.
- `sync` becomes load-bearing; `SyncError` joins the per-stage error set already
  unified by `PipelineError::Stage`.
- `hollywood-assemble` grows a multi-track, offset-aware entry that takes
  per-source (asset, regions, offset); the current single-source `assemble`
  becomes the N = 1 case rather than a separate path.
- The probe/decode front (#72) and the CLI/GUI extend from one input to several;
  the anchor is a new explicit input the caller supplies.
- New domain vocabulary — **sync anchor**, **union cut** — enters
  [`docs/glossary.md`](../docs/glossary.md) with the implementation, distinct
  from 0008's pipeline state types, which stay internal.
- Drift correction and a smarter cut (below) are refinements on these state
  types, not retrofits.

## Alternatives considered

- **Reference-driven cut** — detect on one chosen source and apply its regions
  to all. Simpler (no region union; detection runs once), and fine when one
  source carries the whole scene. But it gates every other source's unique
  speech as silence, so it clips cross-talk and deletes half of any alternating
  conversation — wrong for the interview/podcast case this tool targets.
  Rejected as the default; the union reduces to it when one source dominates.
- **Per-source independent cuts** — trim each source by its own silence. Each
  track is internally tight, but the tracks no longer share a cut, so they
  desync — defeating the point of syncing them. Rejected.
- **Sync before detect (reorder the stages)** — run detection on a single
  post-sync shared timebase. It removes the per-source detect, but reorders
  `PipelineStage::ORDER` and is unnecessary: per-source detection reads only
  that source's samples and the union happens in assembly. Rejected to preserve
  the stage vocabulary.
- **Symmetric pairwise sync (no anchor)** — align all N×N pairs with no
  distinguished clock. More correlation work and no natural timebase or cut
  driver; a single anchor is simpler and sufficient. Rejected.
- **One offset only, never drift** — adequate for short takes; a long-take clock
  drift would smear sync toward the tail. Drift is therefore kept as an explicit
  refinement on offset application rather than ruled out.
