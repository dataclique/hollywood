# 0001 — Own timeline IR + hand-written XML adapters

## Status

Accepted.

## Context

Hollywood must model a multi-track timeline and export it to several NLE
interchange formats (see [0002](./0002-nle-format-strategy.md)). The obvious
"reuse a library" paths do not exist in Rust:

- **OpenTimelineIO's Rust bindings are abandoned** — the `opentimelineio` crate
  is a 2020 placeholder stub and the vfx-rs sys bindings are incomplete and dead
  since ~2022. Taking them as a runtime dependency is not viable, and FFI to
  OTIO's C++ would compromise the single-Rust-codebase goal.
- **No production-quality Rust crate exists** for FCPXML, FCP7 xmeml, or AAF
  read/write.

## Decision

Define Hollywood's own **timeline IR** in `crates/hollywood-timeline`, modeled
to make invalid states unrepresentable (exact rational time, clips bounded by
their source, transitions only in valid positions). Serialize it to each NLE
format with **hand-written emitters** built on `quick-xml`, one adapter per
format.

Borrow OTIO's _data model_ (Timeline/Stack/Track/Clip/Gap + RationalTime/
TimeRange) as a proven design pattern — not as a dependency.

## Consequences

- Full control over the model and over each format's quirks; correctness is
  enforced by golden-file tests rather than trusting a black-box library.
- More code to own and maintain than a library would require.
- A clean seam: every subsystem speaks the IR, so formats and backends are
  projections that can change without touching the core.
- Optional `.otio` export becomes a native `serde_json` projection against a
  pinned OTIO JSON Schema, with the Python OTIO package usable only as a CI
  validation oracle.

## Alternatives considered

- **OTIO Rust bindings** — abandoned/incomplete; rejected.
- **A generic timeline library** — none exists for Rust at production quality.
- **Per-format ad-hoc models** — rejected; it would duplicate logic and break
  the single-source-of-truth the pipeline needs.
