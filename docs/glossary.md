# Glossary

Domain terms used across Hollywood's code, docs, and issues. Keep this in sync
when new concepts appear (see [AGENTS.md](../AGENTS.md)).

## Workflow

- **Pre-edit** — the mechanical preparation before the creative edit: removing
  dead air, syncing audio, and assembling a rough timeline. Hollywood's whole
  job. The opposite of the _creative edit_, which stays with the human.
- **Dead air** — stretches of footage with no useful content: silence or
  non-speech (room tone, pauses). What Hollywood trims.
- **Rough assembly / rough cut** — the trimmed, synced, roughly-ordered timeline
  Hollywood exports as a starting point. Not a finished edit.

## Timeline model

- **Timeline IR** — Hollywood's internal _intermediate representation_ of a
  multi-track timeline (crate `hollywood-timeline`). The single model every
  subsystem reads and writes; NLE formats are projections of it.
- **Rational time** — a time value stored as an exact `i64/i64` rational number
  of seconds, never floating-point. Prevents drift and rounding errors across
  frame rates. (See OTIO's `RationalTime`.)
- **Time range** — a start (rational time) + duration; the in/out window of a
  clip into its source, or a region on the timeline.
- **Track** — an ordered lane of clips and gaps, video or audio.
- **Clip** — a placed reference to a media asset over a time range.
- **Gap** — empty space on a track (used instead of moving clips, to keep
  positions explicit).
- **Media asset** — a source media file with probed properties (duration, frame
  rate, sample rate, channel layout). Clips relink to assets by stable identity.
- **Transition** — a join between adjacent clips. Hollywood targets **hard
  cuts** (no transition) first and **audio cross-fades** as a staged, riskier
  feature.
- **Cross-fade** — an overlap where one clip's audio fades out as the next fades
  in. Notoriously fragile across NLE formats.

## NLE interchange

- **NLE** — Non-Linear Editor: DaVinci Resolve, Final Cut Pro, Adobe Premiere
  Pro. The editor's downstream tool.
- **FCP7 xmeml** — the legacy Final Cut Pro 7 XML interchange format (XMEML v5).
  The **one format that imports natively in both Premiere and Resolve**, so
  Hollywood's _primary_ export target. Still supported by Premiere in 2026.
- **FCPXML** — the modern Final Cut Pro X XML format, also read by Resolve.
  **Does not import natively into Premiere.** Hollywood's secondary path for
  Final Cut / Resolve.
- **AAF** — Advanced Authoring Format: a binary (Structured Storage) interchange
  used by Avid and Premiere. **Deliberately not produced** by Hollywood (no
  maintained Rust writer; buggy cross-fades even from Adobe). See
  [ADR 0002](../adrs/0002-nle-format-strategy.md).
- **OTIO** — OpenTimelineIO, an open interchange format/library. Its Rust
  bindings are abandoned; Hollywood borrows its _data model_ as a design pattern
  and may export `.otio` via native serde, but takes no OTIO runtime dependency.
- **Golden file** — a checked-in reference export. Tests assert that the emitter
  produces byte-/structure-equal output, making NLE compatibility a contract.
- **Relinking** — an NLE re-associating timeline clips with media files on disk.
  Fragile; depends on stable clip naming/paths in the export.

## Audio analysis

- **VAD** — Voice Activity Detection: classifying audio as speech vs non-speech.
  Hollywood uses **Silero VAD** (an ONNX model) via the `ort` runtime.
- **Silence detection** — energy-based detection of quiet regions
  (**RMS**/**peak** gating over short windows), complementary to VAD.
- **RMS** — root-mean-square, a measure of average signal energy in a window.
- **Cross-correlation** — sliding-dot-product similarity of two signals; its
  peak gives the time offset that best aligns them. Hollywood's audio-sync
  primitive, computed via FFT (`rustfft`/`realfft`).
- **GCC-PHAT** — Generalized Cross-Correlation with Phase Transform: cross-
  correlation with spectral whitening, robust for room-mic arrays but prone to
  spurious peaks at low SNR. Opt-in, not the default.
- **Drift** — gradual clock divergence between two recording devices, so a
  single fixed offset won't keep them aligned over a long take. Needs a
  piecewise/linear time map.
- **Frame rate / sample rate / channel layout** — video frames per second, audio
  samples per second, and the arrangement of audio channels (mono/stereo/…).
  Core media-asset properties that must survive into the export.

## Build & tooling

- **Crane** — the Nix library that builds the Rust workspace into the
  `hollywood` / `-test` / `-clippy` derivations CI runs.
- **devenv / direnv** — the dev-shell tooling that provisions the toolchain,
  FFmpeg, and `but` on entry.
- **but / GitButler** — the version-control CLI used for all git writes (stacked
  branches → stacked PRs). See `.claude/skills/gitbutler`.
- **apalis** — the job framework orchestrating the pipeline stages, with a
  durable SQLite backend.
