# 0005 — egui/eframe (wgpu) for the desktop GUI

## Status

Accepted.

## Context

Hollywood ships a cross-platform desktop app (macOS, Windows, Linux). The GUI
should stay within the single-Rust-codebase goal and be able to render media
artifacts (waveforms, thumbnails, video frames) efficiently.

- **`egui`/`eframe`** (0.34.x) is an immediate-mode Rust GUI, production-proven
  (e.g. Rerun), with **wgpu** as the default renderer as of 0.34.0 — so GPU
  compositing of video/waveform rendering sits naturally alongside the UI.
- **Tauri** would put the UI in a web frontend, reintroducing a second language
  and toolchain.

## Decision

Build the desktop shell with **`egui`/`eframe` on the wgpu backend**, using
**`rfd`** (async API) for native file dialogs. The async runtime runs alongside
the GUI event loop and never blocks it.

## Consequences

- One language end to end; GPU rendering of media artifacts is straightforward.
- egui/eframe is **pre-1.0** — pin exact versions and budget for API churn each
  minor release; verify the wgpu version eframe re-exports before mixing direct
  wgpu calls.
- File dialogs must use `rfd`'s **async** API; the synchronous dialog freezes
  the egui event loop. Linux needs an XDG portal/GTK at runtime.

## Alternatives considered

- **Tauri** — web frontend means a second language/toolchain; rejected against
  the single-codebase goal.
- **iced** — capable Rust GUI, but egui's immediate-mode model and wgpu
  compositing fit media tooling and are more battle-tested here.
- **Native toolkits (GTK/Qt)** — heavier bindings, weaker cross-platform Rust
  story.
