# 0008 — Threading typed data through the pipeline stages

## Status

Accepted.

## Context

`run_pipeline` (crate `hollywood-pipeline`) sequences `PipelineStage::ORDER`
through a uniform `run_stage` closure, reports `RunProgress` over a `watch`
channel, and fails fast — but it threads **no data** between stages. Its own doc
leaves the data-flow shape open ("a stateful flow may instead want a
context-passing trait"). The roadmap's "stage chain: wire probe → detect → sync
→ assemble → export" item is blocked on settling that shape.

The stages do **not** form a simple linear `Out → In` chain. The real APIs:

- `probe` (`MediaProbe::probe → ProbedMedia → MediaAsset`), `decode`
  (`DecodeAudio::decode_mono → MonoAudio`), and `detect`
  (`keep_regions(&[f32], SampleRate, &SilenceGate) → Vec<TimeRange>`) run **per
  input source**;
- `sync` (`align` / `drift_map`) correlates **across sources**;
- `assemble` (`assemble(name, FrameRate, MediaAsset, &[TimeRange]) → Timeline`)
  **fans in two** upstream outputs today — the probed asset and the detected
  regions — with a third, the sync offsets, once multi-source lands;
- `export` (`to_fcpxml` / `to_xmeml` / `to_otio(&Timeline)`) consumes the
  assembled timeline.

So data **accumulates and fans in**; "feed stage N's output to N+1" is too weak.
Each stage owns a distinct error (`MediaError` / `DetectError` / `SyncError` /
`AssembleError` / `NleError`), already unified by
`PipelineError::Stage { stage,
source }`.

The decision: what carries the accumulating, fan-in, multi-source typed state,
so that an out-of-order run (assemble before detect) is unrepresentable, without
coupling the stage-agnostic orchestrator to the concrete stage types.

## Decision

Model a run as a chain of **typed states** in a `flow` module of
`hollywood-pipeline`, each an immutable struct holding exactly the artifacts
known so far:

```text
Sources → Probed → Decoded → Detected → Synced → Assembled → Exported
```

Each stage is a typed transform
`fn(PrevState, &Config) → Result<NextState,
StageError>` (async where it does
I/O). The input type proves the prior stages ran, so an out-of-order call cannot
be written. Per-source fan-out/in lives **inside** the transforms and their
state types: `Probed` holds the per-source `MediaAsset`s, `Decoded` the
per-source `MonoAudio` samples that **both** `detect` and `sync` read as
`&[f32]`, `Detected` the regions, and `Synced` the offsets — typed rather than
threaded through a uniform signature. The `Assembled` state carries the
project's `Timeline` IR (ADR 0001), the single hand-off currency `export`
serializes to each NLE format.

The `flow` sequences itself as a straight-line typed composition (`probe(..)?` →
`detect(..)?` → …), so the `flow` — not a uniform per-stage closure — drives the
data. This supersedes `run_pipeline`'s
`run_stage(PipelineStage) → Result<(), E>` shape, which cannot carry a value
whose type changes each step without a union/`Option` carrier (Alternative 1).
What carries over is the orchestration _vocabulary and signals_ `run_pipeline`
established — the `PipelineStage` enum, the `RunProgress` `watch` channel, and
`PipelineError::Stage` — which the `flow` reuses to report progress as it enters
each stage and to wrap each `StageError`. The data-less closure driver can
remain for callers that only need sequencing (tests, a headless dry-run).

A run executes the `flow` **in-process**, threading owned values between
transforms. How that meets durable orchestration — whether the apalis backend
(ADR 0004) treats a run as one job or checkpoints between stages — is decided
with that backend work, not here; this ADR only requires the in-memory state
types, and notes that a checkpointing design would have to serialize the
boundary states.

Config (`SilenceGate`, `DriftWindow`, `CorrelationMethod`, `FrameRate`, export
targets) is passed explicitly to each transform — no hidden defaults. In
particular `FrameRate` is the output sequence's rate, a config input the caller
chooses (it may default to a probed source's rate, but is never silently derived
from one — `assemble(name, FrameRate, MediaAsset, &[TimeRange])` takes it as an
explicit argument).

## Consequences

- Compile-time data contract: each transform's `In`/`Out` types pin the chain; a
  later stage cannot run without the earlier outputs. These state types are
  `hollywood-pipeline`'s own interface — the surface the binary's GUI and CLI
  drive — not new cross-cutting domain vocabulary, so they stay internal to the
  crate rather than entering [`docs/glossary.md`](../docs/glossary.md).
- The orchestration vocabulary and signals (stages, progress,
  `PipelineError::Stage`) are preserved; the in-memory state types stay agnostic
  to how the durable backend (ADR 0004) checkpoints a run, which is decided with
  that work.
- Adding a stage = adding a state + transform; SPEC's optional post-MVP stages
  (auto-framing, audio processing) would slot in as additional states between
  `Assembled` and `Exported` without disturbing the rest. The fan-in stays
  explicit in the `Assembled` transform's signature, and multi-source is modeled
  in the state types from the start rather than retrofitted.
- This ADR fixes the shape only; it does not implement the flow. The stage-chain
  PR follows once accepted.

## Alternatives considered

- **Mutable `RunContext` with `Option` fields** each stage reads and writes —
  simple, and it models the fan-in, but `Option`-laden state makes an
  out-of-order run representable (assemble reads `None` regions). Against the
  project's "make invalid states unrepresentable" rule.
- **A uniform `Stage` trait (`type In; type Out`) chained** — clean for a linear
  chain, but it cannot express the assemble fan-in (two upstream outputs today,
  three with multi-source) or the per-source fan-out without collapsing
  `In`/`Out` into an accumulating state, i.e. it degenerates into the chosen
  state types anyway.
- **One apalis job per stage, threading data through the job store** — would
  force every inter-stage value to be serializable and persisted, heavy for a
  single-user in-process run, and it couples the data flow to the durability
  backend; the typed in-process flow keeps the two concerns separate.
