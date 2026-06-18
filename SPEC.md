# Hollywood — Technical Specification

> Status: living document. This is the authoritative description of what
> Hollywood is and how it is built. Implementation lands in stacked PRs tracked
> in [ROADMAP.md](./ROADMAP.md). Terms in **bold** on first use are defined in
> [docs/glossary.md](./docs/glossary.md).

## 1. Vision

Hollywood automates the **pre-edit**: the mechanical work a video editor does
before the creative edit begins. Given raw footage (and optionally separate
audio recordings), Hollywood:

1. **Removes dead air** — silence and non-speech gaps — so the editor starts
   from tight footage instead of hours of pauses.
2. **Synchronizes** audio across sources (e.g. camera scratch audio and a
   lavalier/recorder track) onto one timeline.
3. **Exports a timeline** that opens directly in the editor's NLE, with the cuts
   and (where supported) audio cross-fades already in place.

The output is a _rough assembly_, not a finished edit. The human keeps full
creative control; Hollywood just deletes the boring part of the workflow.

### What ships in the box

A cross-platform desktop application (macOS, Windows, Linux) plus a CLI for
batch/headless use, both built from one Rust workspace.

## 2. Goals and non-goals

**Goals**

- Open the exported timeline in **DaVinci Resolve**, **Final Cut Pro**, and
  **Adobe Premiere Pro** without manual fix-up of cuts.
- Deterministic, testable output: the same input always produces the same
  timeline, verified against golden files.
- Single Rust codebase from media I/O to GUI — no second-language runtime.
- Run on a single machine with no external services.

**Non-goals**

- Not a finished-edit tool, color grader, or renderer. Hollywood assembles; the
  NLE finishes.
- Not a transcription/subtitle product (though VAD output could feed one later).
- Not a cloud/multi-user service.
- Not an **AAF** producer — see §5.4.
- Not a **video renderer**: auto-framing emits NLE-native keyframed moves, not
  baked pixels (§5.9, ADR 0007). The one rendering exception is corrective
  **audio** stems (§5.8, ADR 0006).

## 3. Architecture

Hollywood is a Cargo workspace. Each capability is its own crate so domain
boundaries stay clean and the dependency graph stays acyclic.

```
         ┌─────────────────────────────────────────────┐
         │            hollywood (app / CLI)             │  egui/eframe + clap
         └───────────────────────┬─────────────────────┘
                                  │
         ┌───────────────────────▼─────────────────────┐
         │              hollywood-pipeline              │  apalis jobs
         │  probe → detect → sync → assemble → export   │
         └───┬───────────┬───────────┬───────────┬──────┘
             │           │           │           │
┌────────────▼──┐ ┌──────▼──────┐ ┌──▼────────┐ ┌▼─────────────┐
│hollywood-     │ │hollywood-   │ │hollywood- │ │hollywood-nle │
│ffmpeg         │ │detect       │ │sync       │ │(FCPXML/xmeml)│
│(media I/O)    │ │(VAD/silence)│ │(xcorr)    │ │              │
└───────────────┘ └─────────────┘ └───────────┘ └──────┬───────┘
                                                        │
                                         ┌──────────────▼───────────────┐
                                         │       hollywood-timeline      │
                                         │   (the timeline IR — core)    │
                                         └───────────────────────────────┘
```

- **`hollywood-timeline`** — the **timeline IR**. The shared vocabulary every
  other crate speaks. Depends on nothing domain-specific.
- **`hollywood-ffmpeg`** — media probe/decode behind a narrow trait, so the
  backend can be swapped (§5.5).
- **`hollywood-detect`** — silence / non-speech detection (§5.2).
- **`hollywood-sync`** — cross-correlation audio alignment (§5.3).
- **`hollywood-nle`** — serializes the IR to NLE formats (§5.4).
- **`hollywood-audio`** — corrective audio post-processing rendered to stems
  (§5.8).
- **`hollywood-reframe`** — content-aware zoom and Ken Burns motion as keyframed
  transforms (§5.9).
- **`hollywood-pipeline`** — orchestrates the stages as jobs (§5.6).
- **`hollywood`** — the desktop app + CLI (§5.7).

### Data flow

`probe` (read media metadata) → `detect` (find keep/cut regions) → `sync` (align
multi-source audio) → `assemble` (build the timeline IR) → optional `reframe`
(add keyframed zoom, §5.9) and `process` (render corrective audio stems, §5.8) →
`export` (serialize to NLE XML). Each stage consumes and produces well-typed
values; the IR is the hand-off currency between `assemble` and `export`.

## 4. The timeline IR

The IR is the heart of the system: an in-memory model of a multi-track timeline,
designed to make invalid states unrepresentable and to be lossily but
predictably projectable onto each NLE format.

Core concepts (modeled as Rust types in `hollywood-timeline`):

- **Rational time** — time as an exact `i64/i64` rational in seconds (à la OTIO
  `RationalTime`), never floating-point frames. Avoids drift and rounding bugs
  across frame rates.
- **Timeline** → ordered **tracks** (video/audio) → ordered **clips** and
  **gaps**. A clip references a **media asset** and an in/out **time range**
  into it, placed at a timeline position.
- **Media asset** — a source file with probed properties (duration, frame rate,
  sample rate, channel layout). Clips relink to assets by stable identity.
- **Transition** — between adjacent clips; initially only audio **cross-fades**
  and hard cuts (§5.4 explains why transitions are a staged risk).
- **Audio-effect chain** — an optional, ordered chain of corrective audio
  effects (normalize / EQ / compress / limit / sidechain-duck) attached to a
  clip or track, realized by `hollywood-audio` (§5.8).
- **Transform** — a video clip's crop / scale / position, **static or
  keyframed** over time, used for auto-framing (§5.9).

Design borrows OTIO's _data model_ as a proven pattern — Timeline/Stack/Track/
Clip/Gap + RationalTime/TimeRange — without taking OTIO as a dependency (§5.4).

## 5. Subsystems

### 5.1 Media I/O (`hollywood-ffmpeg`)

Media is read through **`ffmpeg-next`** (8.1.x, tracking FFmpeg 8). It is chosen
over `rsmpeg` because it is more current and actively maintained and has a far
larger user base; `rsmpeg` has had no commits since 2025-08 and is pinned to
FFmpeg 8.0.

**Honest framing:** `ffmpeg-next` is an FFI wrapper that links FFmpeg's C
libraries via `ffmpeg-sys-next`. Hollywood is therefore a _single Rust codebase_
but **not a C-free binary**, and it carries FFmpeg's LGPL obligations (§6). If a
genuinely dependency-free build is ever required, the fallback is
**Symphonia** + container crates at a much smaller codec/container surface; the
media layer sits behind a trait so this remains possible.

### 5.2 Silence / non-speech detection (`hollywood-detect`)

Two complementary detectors, combined:

- **RMS/peak gating** — cheap energy threshold over short windows, like FFmpeg's
  `silencedetect`. Catches true silence.
- **Silero VAD** — a small ONNX voice-activity model run via **`ort`** (ONNX
  Runtime, currently `2.0.0-rc.12` — no stable release; pin exactly). Catches
  non-speech that is not silent (room tone, noise). Weights come from a wrapper
  bundling the MIT-licensed Silero model. **`webrtc-vad`** is a lighter,
  unmaintained fallback.

Output: keep/cut regions as IR time ranges, with configurable padding so cuts
don't clip speech onsets.

### 5.3 Audio synchronization (`hollywood-sync`)

Aligns multiple recordings of the same event by **cross-correlation** of their
audio envelopes, computed via FFT (**`rustfft`** 6.x through the real-signal
wrapper **`realfft`**).

- Default: plain/normalized cross-correlation — best for device-to-device sync
  of the _same_ source.
- **GCC-PHAT** spectral whitening is available opt-in for genuine room-mic
  material; it is _not_ the default because PHAT amplifies spurious peaks at low
  SNR / in the quiet regions a dead-air trimmer encounters. Guard with
  band-limiting, a denominator epsilon, and a peak-to-sidelobe threshold.
- Long recordings need a piecewise/linear **drift** map, not a single offset.

### 5.4 NLE export (`hollywood-nle`) — the highest-risk subsystem

Hollywood emits **hand-written** XML text. No production-quality Rust crate
exists for any NLE interchange format, so the emitters are built on a pull-based
XML writer (**`quick-xml`**) off the single timeline IR.

**Format strategy (corrected from first-draft assumptions):**

| Format                      | Opens natively in                      | Role in Hollywood                                                                                                            |
| --------------------------- | -------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| **FCP7 `xmeml`** (XMEML v5) | **Premiere Pro _and_ DaVinci Resolve** | **Primary** target — the one format that imports natively in both.                                                           |
| **FCPXML**                  | Final Cut Pro, DaVinci Resolve         | Secondary path for Final Cut / modern Resolve. **Does _not_ import natively into Premiere** (needs a third-party converter). |
| AAF                         | Avid, Premiere                         | **Excluded** (§ below).                                                                                                      |

- **xmeml is the common denominator** and ships first. Premiere still imports
  _and_ exports xmeml as of 2026 (legacy but actively supported). It encodes
  multi-track timelines, hard cuts, and audio cross-fades.
- **Ship hard cuts first; transitions are a separate, riskier deliverable.**
  Cross-fades must be hand-rolled and validated against a real Premiere/Resolve
  import (OTIO's reference fcp_xml adapter lists transitions as unsupported).
- **FCPXML multi-channel audio** is fragile: always emit explicit
  `audioSources`/`audioChannels`/`audioRate` and address channels via
  `audio-channel-source`; pin a target FCPXML version; never claim lossless
  round-trip. A real, unfixed Resolve source-channel conform bug exists — golden
  round-trip tests and a documented manual workaround are required.
- **No AAF.** No maintained pure-Rust AAF writer exists (the one published crate
  is unproven), AAF is binary Structured Storage, and AAF cross-fades are buggy
  even from Adobe's own exporter. XML interchange covers the requirement.
- **OTIO** (`.otio`) is an _optional_ export, implemented as **native Rust
  `serde_json`** against a pinned OTIO JSON Schema version — not via the
  abandoned Rust bindings, and not via a Python runtime. The upstream Python
  `opentimelineio` package may be used only as an offline/CI validation oracle.

Correctness is enforced by a **golden-file** corpus and, where a real NLE is
available, by automated import checks. NLE XML behavior varies by application
version; the golden corpus is the contract.

### 5.5 Backend abstraction

`hollywood-ffmpeg` exposes a narrow trait (probe, decode-to-samples,
decode-frames). The pipeline depends on the trait, not on FFmpeg types, so the
backend can be swapped (e.g. to Symphonia) without touching the pipeline.

### 5.6 Pipeline orchestration (`hollywood-pipeline`)

Stages run as jobs via **`apalis`** (0.7.4) with its **SQLite** backend, giving
durable enqueue and retries that survive a restart — important for long media
jobs. Caveats baked into the design:

- **apalis tracks job state/result, not percent-complete.** Render progress
  comes from Hollywood's own channel (a `tokio::sync::watch`/`broadcast` or a
  SQLite progress column), not from apalis.
- Configure SQLite WAL + `busy_timeout` to avoid `SQLITE_BUSY`.
- The job interface is abstract, so a lighter hand-rolled `sqlx` queue or a
  plain `tokio` task model remains a fallback if apalis proves heavy for
  single-user use.

### 5.7 Desktop app + CLI (`hollywood`)

- GUI: **`egui`/`eframe`** (0.34.x) on the **wgpu** renderer —
  production-proven, lets the UI composite GPU video/waveform/thumbnail
  rendering. Pre-1.0: pin exactly, budget for API churn.
- Native file dialogs: **`rfd`** via its **async** API (the sync dialog freezes
  the egui event loop).
- The async runtime runs _alongside_ the GUI event loop, never blocking it.
- The CLI (clap) exposes the same pipeline for batch/headless/CI use.

### 5.8 Audio post-processing (`hollywood-audio`)

After assembly, an optional stage conditions the cut so the rough assembly is
comfortable to listen to without manual gain-riding. All processing is
**corrective**, not creative; the editor refines further in the NLE. The IR
carries an **audio-effect chain** per clip and per track (vocabulary in
`hollywood-timeline`); `hollywood-audio` realizes it on decoded sample buffers:

- **Loudness normalization** — measure integrated loudness (**EBU R128** / ITU-R
  BS.1770, via `ebur128`) per clip and per track, apply gain toward a target
  **LUFS**.
- **Auto-EQ** — derive a corrective parametric EQ from the average spectrum of a
  clip (or a section, or a track), FFT via `rustfft`/`realfft`, to tame
  resonances and tilt.
- **Dynamics** — a compressor and brick-wall limiter per audio track to hit a
  target integrated LUFS without exceeding a **true-peak** ceiling.
- **Sidechain ducking** — duck music/background tracks by the envelope of the
  main (speech) track when it crosses a threshold.

**Rendered stems, not NLE-native filters (ADR 0006).** NLE audio-filter
interchange is even less reliable than cross-fades, so the stage renders the
chain to new **audio stem** files and the export references those; originals
stay relinkable and the stage is opt-in. Rendering corrective audio is the only
exception to "Hollywood assembles, the NLE finishes" (§2) — never video.

### 5.9 Auto-framing & motion (`hollywood-reframe`)

Once clips are assembled into one scene, an optional stage adds camera-style
motion the editor would otherwise keyframe by hand:

- **Content-aware zoom** — build an inter-frame **activity map** over a clip and
  crop tighter onto the region that actually changes.
- **Ken Burns auto-zoom** — when a clip exceeds a duration threshold and scene
  activity is low, generate a slow keyframed zoom in/out across it instead of a
  static crop.

This needs **decoded video frames** (downscaled) — a new capability on the
`hollywood-ffmpeg` trait (§5.5) beyond audio decode.

**NLE-native keyframed transforms, not baked pixels (ADR 0007).** Zoom is a
**transform/crop with keyframes** in the IR, exported as native motion (FCPXML
`adjust-transform`/`adjust-crop`, FCP7 xmeml basic-motion keyframes) so the
editor can tweak or delete the move and Hollywood stays a non-renderer.
Transform interchange varies by NLE, so it is golden-file tested and validated
against real imports, with a static-transform fallback.

## 6. Licensing and distribution

- Hollywood is © 2026 Data Clique Software Design FZCO under **BUSL-1.1**
  (converts to GPL-2.0-or-later on the Change Date).
- **FFmpeg** is linked under **LGPL**: build FFmpeg with LGPL configuration,
  ship the required notices and the ability to relink, and **avoid GPL-only
  codecs** (no `x264`/`x265`). Prefer OpenH264 / AV1; treat H.264 distribution
  as needing counsel.
- Bundled model weights (Silero) are MIT; ONNX Runtime ships its own license.

## 7. Key risks

- **NLE XML fragility** (audio fades, multi-channel, relinking) varies by NLE
  version — mitigated by golden-file CI against real installs.
- **Transitions/cross-fades** in xmeml/FCPXML are unproven in Rust — ship hard
  cuts first.
- **Drift and reverberant-mic misalignment** defeat naive single-offset
  cross-correlation — budget for envelope/GCC-PHAT refinement.
- **Pre-1.0 dependencies** (egui, ort, quick-xml, apalis-on-1.0-rc) churn — pin
  exact versions and isolate behind crate boundaries.

See [ROADMAP.md](./ROADMAP.md) for sequencing and [`adrs/`](./adrs) for the
decisions that shaped this spec.
