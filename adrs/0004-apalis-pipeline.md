# 0004 — apalis + SQLite for pipeline orchestration

## Status

Accepted.

## Context

The pipeline (probe → detect → sync → assemble → export) runs long media jobs on
a single-user desktop. Restarting mid-job and losing all progress is
unacceptable, so some durability is wanted — but a full broker (Redis, etc.) is
overkill for a local app.

- **`apalis` 0.7.4** with its **SQLite** backend gives durable enqueue and
  retries on top of `sqlx`, with no external service.
- Plain **tokio** tasks give no durability across restarts.
- The polished SQLite ergonomics live on the `apalis` 1.0-rc line (and the
  separate `apalis-sqlite` crate), which is not yet stable.

## Decision

Orchestrate the pipeline with **`apalis` 0.7.4 + SQLite**, behind an **abstract
job interface** in `crates/hollywood-pipeline` so the backend can change.

## Consequences

- Durable enqueue + retries survive an app restart.
- **apalis tracks job state/result, not percent-complete** — render progress
  comes from Hollywood's own channel (`tokio::sync::watch`/`broadcast` or a
  SQLite progress column), not from apalis.
- SQLite must be configured with WAL + `busy_timeout` to avoid `SQLITE_BUSY`.
- The abstract interface keeps a lighter hand-rolled `sqlx` queue, or plain
  tokio, available as a fallback if apalis proves heavy for single-user use.

## Alternatives considered

- **Plain tokio tasks** — no durability across restarts; insufficient alone.
- **Hand-rolled `sqlx`/`rusqlite` job table** — viable and lighter; kept as the
  fallback the abstract interface enables.
- **apalis 1.0-rc + `apalis-sqlite`** — better SQLite ergonomics but
  pre-release; revisit when stable.
- **Redis/external broker** — overkill for a single-user desktop app.
