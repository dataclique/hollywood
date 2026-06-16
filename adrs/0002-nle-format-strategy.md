# 0002 — NLE format strategy: xmeml primary, no AAF

## Status

Accepted.

## Context

The hard requirement is that an exported timeline opens in **both Adobe Premiere
Pro and DaVinci Resolve** (and ideally Final Cut). The interchange landscape, as
verified in mid-2026:

- **FCP7 xmeml** (XMEML v5) imports **natively in both Premiere and Resolve**
  and is still actively supported by Premiere in 2026. It encodes multi-track
  timelines, hard cuts, and audio cross-fades.
- **FCPXML** is read by Final Cut and Resolve but **does not import natively
  into Premiere** (it needs a third-party converter).
- **AAF** is binary Structured Storage with no maintained pure-Rust writer; its
  cross-fade export is buggy even from Adobe's own exporter.

## Decision

- **FCP7 xmeml is the primary export target** — the single format that satisfies
  "opens in both Premiere and Resolve."
- **FCPXML is a secondary path** for Final Cut and modern Resolve, not a
  Premiere path.
- **Do not produce AAF.**
- **Ship hard cuts first.** Audio cross-fades are a separate, riskier
  deliverable validated against real NLE imports (reference adapters list
  transitions as unsupported).
- Emit explicit `audioSources`/`audioChannels`/`audioRate` in FCPXML and pin a
  target format version; never claim lossless multi-channel round-trip.

## Consequences

- One format (xmeml) covers the core requirement, reducing surface area.
- Transitions and FCPXML multi-channel are known-fragile and gated behind golden
  tests and real-import checks; a documented Resolve source-channel workaround
  is required.
- Users needing Avid/AAF are out of scope.

## Alternatives considered

- **AAF** — no maintained Rust writer, binary format, buggy cross-fades;
  rejected.
- **FCPXML as the primary/only format** — rejected: it does not import natively
  into Premiere, which violates the core requirement.
- **One file for all NLEs** — rejected: NLE behavior diverges too much; separate
  adapters per target are more reliable.
