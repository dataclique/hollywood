# Glossary

Domain terms used across Hollywood's code, docs, and issues. Keep this in sync
when new concepts appear (see [AGENTS.md](../AGENTS.md)).

## Workflow

- **Pre-edit** — the mechanical preparation before the creative edit: removing
  dead air, syncing audio, and assembling a rough timeline. Hollywood's whole
  job. The opposite of the _creative edit_, which stays with the human.
- **Dead air** — stretches of footage with no useful content: silence or
  non-speech (room tone, pauses). What Hollywood trims.
- **Keep / cut regions** — the spans of a take to retain vs drop. Silence
  detection (crate `hollywood-detect`) splits a recording into **keep regions**
  (speech, padded so onsets and tails are not clipped) and the complementary
  **cut regions** (dead air); the assembler trims to the keep regions.
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
- **Audio-effect chain** — an optional, ordered set of corrective audio effects
  (normalize / EQ / compress / limit / sidechain-duck) attached to a clip or
  track in the IR; realized by `hollywood-audio` (see
  [ADR 0006](../adrs/0006-audio-post-processing-stems.md)).
- **Transform** — a video clip's crop / scale / position; **static or
  keyframed** over time. How Hollywood expresses auto-framing in the IR and
  export (see [ADR 0007](../adrs/0007-auto-framing-native-transforms.md)).
- **Keyframe** — a (time, value) control point; the NLE interpolates between
  keyframes to animate a transform (or audio level) over a clip.

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
- **dBFS** — decibels relative to full scale, where `0 dBFS` is the maximum
  sample amplitude (`1.0`). Silence thresholds are negative (e.g. `-40 dBFS`);
  the gate labels a window silent when its RMS falls below the threshold.
- **Cross-correlation** — sliding-dot-product similarity of two signals; its
  peak gives the time offset that best aligns them. Hollywood's audio-sync
  primitive, computed via FFT (`rustfft`/`realfft`).
- **GCC-PHAT** — Generalized Cross-Correlation with Phase Transform: cross-
  correlation with spectral whitening, robust for room-mic arrays but prone to
  spurious peaks at low SNR. Opt-in, not the default.
- **Sync offset** — the signed offset (in samples / rational time) by which one
  recording lags another, recovered from the correlation peak so multi-source
  clips can sit on a shared timebase. Positive means the target starts later.
- **Drift** — gradual clock divergence between two recording devices, so a
  single fixed offset won't keep them aligned over a long take. Needs a
  piecewise/linear time map.
- **Drift map** — the sync offset sampled window-by-window across a recording,
  so a drifting clock shows as an offset that changes over time. Each window is
  measured as the small **residual** around a coarse **base** offset (from one
  whole-take cross-correlation), so sources started far apart need no wider a
  window than tightly-aligned ones; the consumer interpolates across windows
  that carry no correlatable content.
- **Frame rate / sample rate / channel layout** — video frames per second, audio
  samples per second, and the arrangement of audio channels (mono/stereo/…).
  Core media-asset properties that must survive into the export.

## Audio processing

Post-cut conditioning of the assembled audio (crate `hollywood-audio`, §5.8).
All **corrective**, rendered to stems
([ADR 0006](../adrs/0006-audio-post-processing-stems.md)).

- **LUFS** — Loudness Units relative to Full Scale: the perceptual loudness unit
  Hollywood normalizes toward. **Integrated LUFS** is the loudness over a whole
  clip/track.
- **EBU R128 / ITU-R BS.1770** — the standard loudness-measurement algorithm
  behind LUFS, measured via the `ebur128` crate.
- **True peak** — the real inter-sample peak of a signal (above sample peaks);
  the ceiling a limiter must not exceed to avoid downstream clipping.
- **Normalization** — applying a gain so a clip/track hits a target integrated
  LUFS.
- **Parametric EQ** — frequency-selective gain (per-band center / width / gain).
  Hollywood derives a **corrective** EQ from a clip's average spectrum.
- **Compressor / limiter** — dynamics processors that reduce loud peaks (a
  limiter being a hard, fast compressor) to control dynamic range and hit a
  loudness target under a true-peak ceiling.
- **Sidechain ducking** — automatically lowering one track (music/background)
  driven by the level of another (the speech track) — the "duck music under
  talking" effect.
- **Stem** — a rendered audio file with processing baked in, referenced by the
  timeline as an ordinary clip (so no fragile NLE-native filters cross over).

## Framing & motion

Camera-style motion added after assembly (crate `hollywood-reframe`, §5.9),
exported as native keyframed transforms
([ADR 0007](../adrs/0007-auto-framing-native-transforms.md)).

- **Activity map** — a per-region measure of how much a clip's frame changes
  over its duration (inter-frame difference); drives where to crop.
- **Content-aware zoom** — choosing a tighter crop onto the active region of the
  frame, from the activity map.
- **Ken Burns** — a slow, steady zoom/pan across an otherwise static shot; here,
  auto-applied to long, low-activity clips as a keyframed transform.

## Pipeline

The staged run that drives footage to an export (crate `hollywood-pipeline`).

- **Pipeline** — the staged run that turns footage into an export, one **stage**
  at a time: **probe → detect → sync → assemble → export**. Stages run in order
  and fail-fast, behind an abstract job interface so the durable backend can
  change ([ADR 0004](../adrs/0004-apalis-pipeline.md)).
- **Progress channel** — Hollywood's own run-progress signal (a
  `tokio::sync::watch`), separate from apalis's job-state tracking, that the
  desktop app and CLI render. apalis tracks _job state_, not percent-complete.

## Build & tooling

- **Crane** — the Nix library that builds the Rust workspace into the
  `hollywood` / `-test` / `-clippy` derivations CI runs.
- **devenv / direnv** — the dev-shell tooling that provisions the toolchain,
  FFmpeg, and `but` on entry.
- **but / GitButler** — the version-control CLI used for all git writes (stacked
  branches → stacked PRs). See `.claude/skills/gitbutler`.
- **apalis** — the job framework orchestrating the pipeline stages, with a
  durable SQLite backend.
