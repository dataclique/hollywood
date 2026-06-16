# Contributing to Hollywood

Thanks for your interest in Hollywood. This guide covers how to get set up and
how changes land. The detailed engineering rules — code style, testing, quality
gates — live in [AGENTS.md](./AGENTS.md), which applies to human contributors
and AI agents alike.

## Getting started

Hollywood builds with [Nix](https://nixos.org). Install Nix (with flakes
enabled), then:

```bash
# With direnv (recommended): drops the toolchain + `but` on PATH automatically.
direnv allow

# Or enter the dev shell manually:
nix develop --impure --accept-flake-config
```

The dev shell provides the Rust toolchain, FFmpeg, and the GitButler CLI. You do
not need to install Rust, FFmpeg, or any other dependency yourself — the flake
pins everything.

## The development loop

```bash
cargo check     # fast compile check while iterating
cargo test      # run the test suite
cargo clippy    # lint — CI denies all warnings
cargo fmt       # format before committing
```

Before pushing, the CI-equivalent gates are:

```bash
nix build .#hollywood-clippy .#hollywood-test
nix flake check
```

We practice type-driven TDD: model the domain in types, write a failing test,
then implement. See [AGENTS.md](./AGENTS.md#testing) for the testing approach
(golden-file tests for the NLE exporters, realistic media fixtures).

## How changes land

- **Version control is [GitButler](https://gitbutler.com) (`but`)**, not raw
  `git` for writes. The dev shell installs the gitbutler skill/reference.
- **Small, stacked PRs.** One PR per branch, smallest reviewable diff, ideally
  500–1000 lines. Stack with `but branch new <child> --anchor <parent>`.
- **Every PR closes a tracking issue** and ticks its box in
  [ROADMAP.md](./ROADMAP.md), updated on the PR itself.
- **`master` is protected.** Everything lands through PRs; open them as drafts
  until ready for review. Fill in the PR template
  ([`.github/PULL_REQUEST_TEMPLATE.md`](./.github/PULL_REQUEST_TEMPLATE.md)).
- **Keep docs in lockstep.** If your change makes a doc untrue, fix it in the
  same PR.

## License

By contributing, you agree that your contributions are licensed under the
[Business Source License 1.1](./LICENSE) that governs this project. Hollywood is
© 2026 Data Clique Software Design FZCO.
