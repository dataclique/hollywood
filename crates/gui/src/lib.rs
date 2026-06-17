//! Hollywood desktop shell — `egui`/`eframe` on wgpu (ADR 0005).
//!
//! Runnable first slice: pick footage, probe it via [`hollywood_ffmpeg`], choose
//! export targets, and show a (stub) progress bar until the pipeline lands.

mod app;
mod export;
mod footage;
mod picker;

pub use app::run;
