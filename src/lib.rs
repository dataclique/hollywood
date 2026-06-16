//! Hollywood: pure-Rust video pre-editing automation.
//!
//! This crate is the application root. At the foundation stage it establishes
//! the two pieces every later subsystem builds on: the FFmpeg-backed media
//! backend ([`media`]) and the persistent job queue ([`JobQueue`]) that the
//! processing pipeline will drive. The timeline IR, NLE exporters, audio
//! sync/detection, and the `egui` desktop shell arrive in dedicated workspace
//! crates.

pub mod media;

use thiserror::Error;

/// Top-level error type for the Hollywood application root.
#[derive(Debug, Error)]
pub enum Error {
    /// The FFmpeg-backed media subsystem failed to initialize.
    #[error("media backend initialization failed: {0}")]
    Media(#[from] ffmpeg_next::Error),
}

/// Persistent, single-file job queue backing the processing pipeline.
///
/// Aliased at the workspace root so every crate shares one storage backend.
/// SQLite keeps the desktop app self-contained — no external broker — while
/// still giving apalis durable enqueue, retries, and progress. The pipeline
/// crate supplies the concrete job types `J`.
pub type JobQueue<J> = apalis_sql::sqlite::SqliteStorage<J>;
