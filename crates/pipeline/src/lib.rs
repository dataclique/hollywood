//! Pipeline orchestration for Hollywood.
//!
//! The pipeline turns raw footage into an exported timeline through an ordered
//! chain of [`PipelineStage`]s: probe → detect → sync → assemble → export.
//! [`run_pipeline`] drives that chain — running each stage in turn, stopping at
//! the first failure, and publishing [`RunProgress`] over a
//! [`ProgressSubscription`] the desktop app and CLI can render.
//!
//! Per [ADR 0004](../adrs/0004-apalis-pipeline.md), durable orchestration will
//! be an `apalis` + SQLite backend behind an abstract job interface (with a
//! tokio/`sqlx` fallback), and progress is Hollywood's own signal because apalis
//! tracks job state, not percent-complete. This module is the backend- and
//! stage-agnostic core: the stage vocabulary, the progress channel, and the
//! fail-fast sequencing, with the concrete per-stage work supplied by the caller.

mod error;
mod orchestrate;
mod progress;
mod stage;

pub use error::PipelineError;
pub use orchestrate::run_pipeline;
pub use progress::{ProgressReporter, ProgressSubscription, RunProgress};
pub use stage::PipelineStage;
