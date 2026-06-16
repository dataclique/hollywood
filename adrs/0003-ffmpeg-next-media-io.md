# 0003 — `ffmpeg-next` for media I/O

## Status

Accepted.

## Context

Hollywood must probe and decode a broad range of real-world media (containers,
codecs, sample rates, channel layouts) to drive detection and sync. The options:

- **`ffmpeg-next`** (8.1.x, tracking FFmpeg 8) — higher-level safe abstractions
  over FFmpeg; self-described "maintenance-only" but in practice the _more_
  current and active of the FFmpeg wrappers (last push 2026-06), ~4.27M
  downloads.
- **`rsmpeg`** — thinner, lower-level FFmpeg wrapper; pinned to FFmpeg 8.0, no
  commits since 2025-08.
- **Symphonia** — genuinely pure-Rust demux/decode, but a much smaller
  codec/container surface.

Both FFmpeg wrappers are unsafe FFI that link FFmpeg's C libraries — neither
yields a pure-Rust binary.

## Decision

Use **`ffmpeg-next` 8.x** for media I/O, accessed through a narrow trait in
`crates/hollywood-ffmpeg` (probe, decode-to-samples, decode-frames). The
pipeline depends on the trait, not on FFmpeg types.

## Consequences

- Broad, battle-tested format support immediately.
- Hollywood is a single Rust codebase but **not a C-free binary**: it links
  FFmpeg's C libraries and carries **LGPL** obligations (LGPL build config,
  notices, relink capability; avoid `x264`/`x265`). The Nix dev shell pins the
  FFmpeg libraries so the link is reproducible.
- "Maintenance-only" means new FFmpeg APIs may require upstreaming a PR — budget
  for it.
- The trait boundary keeps **Symphonia** viable as a fallback if a pure-Rust
  build is ever required, at a narrower format surface.

## Alternatives considered

- **`rsmpeg`** — stale and pinned to FFmpeg 8.0; rejected.
- **Symphonia only** — pure Rust but too narrow a codec/container surface for
  arbitrary user footage today; kept as a fallback behind the trait.
- **Shelling out to the `ffmpeg` binary** — rejected: fragile parsing, process
  overhead, and a runtime dependency to locate.
