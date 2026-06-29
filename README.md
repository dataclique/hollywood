# Hollywood

Hollywood is a pure-Rust video **pre-editing automation** tool. It ingests raw
footage, detects and removes dead air (silences / non-speech), synchronizes
audio from multiple sources, and emits a ready-to-edit timeline that opens in
both **DaVinci Resolve / Final Cut** (via FCPXML) and **Adobe Premiere Pro**
(via FCP7 `xmeml` XML) — so the creative edit starts from an assembled, trimmed
sequence instead of a pile of clips.

It is built around its own timeline intermediate representation (IR) with
hand-written XML adapters, `ffmpeg-next` for media I/O, `rustfft` for
cross-correlation audio sync, RMS/peak + Silero VAD for silence detection, and
`egui`/`eframe` for the desktop GUI.

> **Status:** the analysis and export building blocks are in the tree — the
> timeline IR, both NLE exporters (FCP7 `xmeml`, including audio cross-fades,
> and FCPXML) plus optional `.otio`, the FFmpeg probe and audio-decode layer,
> RMS silence detection, audio sync (cross-correlation and GCC-PHAT, with a
> piecewise drift map for long takes), the `assemble` step that lays keep
> regions into a trimmed timeline, and the pipeline-orchestration skeleton. The
> `egui` desktop shell is runnable for picking footage and choosing export
> targets. Exporters are verified against golden files; native import into
> Resolve/Premiere is not yet validated. Still to come: wiring the stages into
> one end-to-end pipeline, Silero VAD, a durable job backend, and the CLI. See
> [`SPEC.md`](./SPEC.md) and [`ROADMAP.md`](./ROADMAP.md).

## Running

```bash
cargo run              # opens the desktop shell (default)
cargo run -- init      # headless smoke check for CI
```

## Development

The repository is a [Nix](https://nixos.org) flake with a
[devenv](https://devenv.sh) dev shell. With [direnv](https://direnv.net)
installed:

```bash
direnv allow   # enters the dev shell, puts the toolchain + `but` on PATH
```

Without direnv, enter the shell manually:

```bash
nix develop --impure --accept-flake-config
```

### Common commands

```bash
cargo build              # build the workspace
cargo test               # run the test suite
cargo clippy             # lint (CI denies all warnings)
cargo fmt                # format

nix build .#hollywood          # release build via crane
nix build .#hollywood-test     # test derivation (CI parity)
nix build .#hollywood-clippy   # clippy derivation (CI parity)
nix flake check                # run pre-commit hooks + checks
```

Version control uses the [GitButler](https://gitbutler.com) CLI (`but`),
provisioned by the [`but.nix`](https://github.com/dataclique/but.nix) flake
input and available on the dev-shell `PATH`. See the gitbutler agent skill
installed into `.claude/skills/gitbutler`.

## License

Hollywood is © 2026 Data Clique Software Design FZCO and distributed under the
[Business Source License 1.1](./LICENSE). It converts to GPL v2.0-or-later on
the Change Date. For commercial licensing, contact Data Clique Software Design
FZCO.
