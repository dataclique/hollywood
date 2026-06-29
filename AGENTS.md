# AGENTS.md

Rules and guidelines for AI agents and human contributors working in this
repository. Everything in this document is a directive, not a suggestion.

---

## Project Direction

Hollywood is a video **pre-editing automation** tool. It ingests raw footage,
removes dead air (silence / non-speech), synchronizes audio from multiple
sources, and emits an assembled, trimmed timeline that opens natively in both
**DaVinci Resolve / Final Cut** (FCPXML) and **Adobe Premiere Pro** (FCP7
`xmeml` XML). The creative edit then starts from a rough cut instead of a pile
of clips.

See [SPEC.md](./SPEC.md) for the vision and [ROADMAP.md](./ROADMAP.md) for the
path. For status — what works today vs. what is planned — see
[README.md](./README.md).

### Agent expectations

Agents working in this repo are expected to:

- Read [ROADMAP.md](./ROADMAP.md) and the relevant tracking issue before
  changing code. The issue is the contract.
- Follow type-driven TDD: types first, a failing test, then implementation.
- Honor the rules in this document for code style, testing, and quality gates.
- Edit code, tests, docs, and configs in this repo. Humans own releases,
  signing/notarization, secrets, and anything outside git.
- Never relax a quality check (clippy, the test suite, the pre-commit hooks)
  without explicit permission. Ask if a check seems wrong; do not suppress it.
- Not substitute approaches, libraries, or tools without checking in. Scope is
  what was asked, not what you would prefer.

### Key architectural decisions

These are settled. Revisit them only with an ADR (see [`adrs/`](./adrs)) and
explicit sign-off.

- **One Rust codebase, no second-language runtime.** The whole pipeline —
  timeline model, exporters, detection, sync, GUI — is Rust. There is **no
  Python/OTIO runtime dependency**. (Hollywood is not a C-free binary: it links
  FFmpeg's LGPL C libraries through `ffmpeg-next` for media I/O. "Rust" here
  means a single Rust codebase, not the absence of system C libraries.)
- **Own the timeline IR; hand-write the XML adapters.** Hollywood has its own
  timeline intermediate representation and serializes it to two hand-written
  text adapters: **FCPXML** (Resolve / Final Cut) and **FCP7 xmeml** (Premiere).
  We do not depend on OpenTimelineIO's Rust bindings (abandoned) at runtime.
- **Route around AAF.** No maintained pure-Rust AAF writer exists and AAF
  cross-fades are unreliable even from Adobe's own exporter. FCPXML + xmeml
  cover "must open in both Premiere and Resolve" without AAF.
- **`ffmpeg-next` for media I/O** (tracks FFmpeg 8.x; actively maintained,
  unlike `rsmpeg`). Keep the media layer behind a `hollywood-ffmpeg` boundary so
  the backend can be swapped if the filter-graph API proves limiting.
- **`rustfft` cross-correlation for audio sync; RMS/peak + Silero VAD (via
  `ort`) for silence detection; `egui`/`eframe` (wgpu) for the desktop GUI;
  `apalis` (SQLite backend) for pipeline orchestration** behind an abstract job
  interface so a `tokio`-task fallback stays possible.

---

## Development

### Environment

The repo is a Nix flake with a [devenv](https://devenv.sh) dev shell. With
[direnv](https://direnv.net): `direnv allow` activates it and puts the Rust
toolchain, FFmpeg, and the GitButler CLI (`but`) on `PATH`. Without direnv:
`nix develop --impure --accept-flake-config`.

All toolchain and system dependencies are managed through the flake — never
`cargo install`, `brew install`, or similar.

### Commands

```bash
cargo check              # fast type/compile verification
cargo test               # run the test suite
cargo clippy             # lint (CI denies all warnings)
cargo fmt                # format Rust

nix build .#hollywood          # release build via crane
nix build .#hollywood-test     # test derivation (CI parity)
nix build .#hollywood-clippy   # clippy derivation (CI parity)
nix flake check                # pre-commit hooks + checks
```

**Type-driven TDD (TTDD).** 1) Define the types/traits/signatures that model the
domain. 2) Write a test that compiles but fails (a build error is not a failing
test). 3) Implement until it passes. Iterate `cargo check`/`cargo test` while
developing; run `cargo clippy` and `cargo fmt` before committing.

**Dependencies.** Add crates with `cargo add <crate>` — never hand-write version
numbers (they get hallucinated). Shared dependencies live in the
`[workspace.dependencies]` catalog in the root `Cargo.toml`; crates reference
them with `<dep>.workspace = true` (per the dotted-vs-block guidance under
[Code Style](#code-style)).

### Version control

All write operations go through the GitButler CLI (`but`) — never `git add`,
`git commit`, `git push`, `git checkout`, `git rebase`, or other git writes.
Read-only git inspection (`git status`, `git log`, `git diff`) is fine. See the
gitbutler skill at `.claude/skills/gitbutler/SKILL.md` for the full command
reference.

---

## Workflow & Policies

### When issues are pointed out

Fix immediately. The user does not send messages for the sake of it.

### Attribution

Never add "Generated with …" or self-credit to commits, PRs, issues, or code.
Never speak on the user's behalf through their accounts (GitHub comments, review
state, chat). Opening/editing PRs and issues you were told to open is fine;
commenting and review-state changes are not, absent an explicit instruction.

### PR descriptions

Explain WHY the change exists and what behavior changed — not a narrative of
your process. Follow the section shape in
[`.github/PULL_REQUEST_TEMPLATE.md`](./.github/PULL_REQUEST_TEMPLATE.md). One
line per bullet; detail belongs in the code or the linked issue.

### Every PR is tracked by an issue and the roadmap

Every PR `Closes` a problem-only GitHub issue, and that issue is a checklist
item in the relevant [ROADMAP.md](./ROADMAP.md) section linking the issue and
the PR. Keep the roadmap in lockstep: the entry and its tick land on the feature
PR itself, so the roadmap always matches what merged.

### Stacked PRs

Work lands as small, stacked PRs — one PR per branch, smallest reviewable diff,
target 500–1000 lines of additions. Stack with
`but branch new <child> --anchor <parent>`. Each branch must be independently
buildable. `master` is **protected**: never push to it directly; everything
lands through PRs. Open PRs as **drafts** until they are ready for review.
GitButler writes a stack-navigation footer into stacked PRs; keep it current.

### Documentation stays in lockstep with the code

Every PR must leave the docs in a true state. Before handing off, audit what you
touched:

- [README.md](./README.md): does it still describe the system and its status?
- [SPEC.md](./SPEC.md): does the vision/architecture still match the code?
- [ROADMAP.md](./ROADMAP.md): completed items marked, new ones listed, stale
  ones removed?
- [`docs/glossary.md`](./docs/glossary.md): are new domain terms defined?
- [`adrs/`](./adrs): did a settled decision change? Amend or supersede the ADR.
- This file: did a rule change in practice? The rule changes here first.
- Inline doc comments on any code you touched.

Stale documentation is a bug. Do not ship work that introduces it.

### Quality checks

**Never disable or relax a quality check without explicit permission** —
`#[allow(clippy::*)]`, `#[allow(dead_code)]`, `#[allow(unused)]`, or weakening
the lint config. Fix the underlying code. If suppression is genuinely correct (a
false positive, or a lint that conflicts with policy), STOP and ask; when
granted, add a comment explaining why.

### No hidden defaults

Never add default values (`#[serde(default)]`, `unwrap_or(...)`) without being
asked. Required configuration should fail loudly when missing, not silently use
a value the user did not choose.

### When stuck

If a fix does not work after three attempts, read the official documentation
before trying a fourth.

---

## Code Style

The workspace enforces a strict lint policy (root `Cargo.toml`): `unsafe_code`
is **forbidden**; `unwrap`, `expect`, `panic`, `unreachable`, `unimplemented`,
and `indexing_slicing` are **denied** outside tests; `clippy::pedantic` and
`clippy::nursery` are denied. Write code that passes without suppression.

- **Functional and type-driven.** Prefer pure functions and immutable data. Use
  the type system to make invalid states unrepresentable — a malformed timeline,
  a clip outside its track's bounds, or a negative duration should not be
  constructible.
- **Iterator-first, immutable by default.** Express a transformation as an
  iterator chain rather than a mutable accumulator: a build-then-`push` loop is
  `.map(..).collect()` / `.extend(..)`; a running total is `.fold` / `.sum` /
  `.try_fold`; index arithmetic with a bounds-checked lookup is `.enumerate()` /
  `.zip()` / `.skip()` / `.windows()`; fallible elements are
  `.collect::<Result<_, _>>()`, never a chain that silently drops the error.
  Reach for a `let` binding over `let mut` whenever an expression or chain
  produces the value. This is a readability rule, not a dogma: keep a `for` loop
  where it carries genuine state across iterations, drives `.await`, emits I/O,
  or breaks early on a condition a combinator would obscure — a stateful demux
  loop, the async stage orchestrator, and an XML-emit loop all read clearer
  imperative. Convert only when the iterator form is both behavior-identical and
  genuinely clearer; a change that just shuffles a loop into a combinator
  without improving it is overhead.
- **No boolean blindness.** Prefer discriminated unions
  (`enum Fade { In, Out }`) over bare `bool`s; when a boolean is unavoidable,
  wrap it in named constructors rather than exposing `set_x(true/false)`.
- **Model the value, not a `String`.** Every identifier and domain value gets a
  type that captures its actual structure — a newtype wrapping `String` that
  encodes nothing is still a bug, and a type alias (`type X = Y`) is not a type
  at all (the compiler can't tell it apart). Choose the shape by what determines
  the value:
  - opaque and randomly generated → wrap a `Ulid`/`Uuid`, not `String`;
  - composed of N values → a struct with those N fields;
  - one of N kinds → an enum with N variants (e.g. `MediaSource::File(PathBuf)`,
    extensible to a URL without touching call sites).

  Put the string conversion on the type itself (`Display`/`FromStr`), defined in
  exactly one place, so formatting and parsing never spread across call sites as
  `format!(...)` and ad-hoc parsers.
- **Newtypes over primitives.** Wrap domain quantities (rational time, frame
  rate, sample rate, track index) in types; don't pass raw `i64`/`f64` where a
  domain type clarifies intent and units.
- **Package by feature, not by layer.** Each capability is a workspace crate.
  The directory drops the project prefix (`crates/timeline`, `crates/nle`,
  `crates/ffmpeg`, …); the Cargo **package name** keeps it
  (`hollywood-timeline`, `hollywood-nle`, …) so it stays unambiguous on
  crates.io and in dependency lists. Keep domain boundaries clean and the
  dependency graph acyclic.
- **Error handling.** Use `thiserror` enums per crate; propagate with `?`. Add a
  variant with `#[from]` for each error type you need to surface — callers then
  use `?` and keep the full error chain. Almost never use `.map_err`: it
  discards information and duplicates variants you already have. The rare
  exception is a boundary you do not control (a trait whose associated `Error`
  type is fixed, a foreign callback signature) where you must convert into an
  existing type — even then, prefer wrapping the underlying error on your enum
  (`#[from]` / `#[source]`) over `.map_err(|_| YourEnum::SomethingGeneric)`. No
  panics in non-test code — model the failure.
- **Module organization — public API first.** Within a module, order code so the
  most important things appear first: public types/traits, then public
  functions/impls, then private helpers. Reviewers should understand the
  interface before the internals.
- **Minimal visibility.** Only a crate's real public API is `pub`. A library
  whose sole consumer is the binary (e.g. `hollywood-gui`, reached only via
  `run`) exposes just that entry point; everything else is `pub(crate)` or
  private. Over-`pub` misrepresents the crate's API surface, and in a library
  crate it also hides bugs: `pub` declares external API, so the compiler exempts
  unused `pub` items from `dead_code` even when nothing outside the crate uses
  them. `pub(crate)` and private get identical `dead_code` analysis, so default
  to the narrowest visibility that compiles. Integration tests under `tests/`
  are the exception: Cargo compiles them as separate crates, so they can only
  call `pub` items — the API they import is genuinely public and stays `pub`;
  reach for `#[cfg(test)]` unit tests in `src/` when you need to exercise
  internals instead of widening visibility.
- **Comments.** Doc comments (how to use the code) are good. Comments narrating
  what the code does are not — make the code clear through naming and structure
  instead.
- **Clear but concise — let the formatter decide the threshold.** The governing
  rule is _clear first, concise second_, judged by how it reads **after the
  formatter runs**, which differs by language:
  - **TOML (taplo):** one key → dotted (`dep.workspace = true`); two or more
    keys → inline table (`dep = { workspace = true, features = [...] }`), which
    taplo keeps on one line.
  - **Nix (nixfmt):** prefer the flattened dotted form (`a.b = x;` `a.c = y;`;
    `services.foo.enable = true`) — nixfmt explodes a multi-attribute `{ … }`
    onto separate lines, so reach for braces only when the grouping genuinely
    aids clarity.

---

## Testing

- **Pyramid.** Many fast unit tests on the timeline IR and the detection/sync
  math; fewer integration tests on the pipeline; a thin layer of golden-file
  tests on the NLE exporters.
- **Golden files for XML export.** FCPXML and xmeml output is verified against
  checked-in golden files, and (where a real NLE is available in CI) against
  Resolve/Premiere import. NLE XML interchange is fragile — audio fades,
  multi-channel audio, and clip relinking vary by NLE version — so the golden
  corpus is the contract.
- **Realistic fixtures.** Reproduce bugs against real media shapes (containers,
  sample rates, channel layouts). A test that hand-constructs invalid state and
  asserts it is invalid proves nothing; exercise the real code path.
- `unwrap`/`expect`/indexing are allowed in tests (see `clippy.toml`).
