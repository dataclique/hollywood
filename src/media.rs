//! Media backend initialization (FFmpeg via `ffmpeg-next`).
//!
//! Hollywood links FFmpeg's libraries directly rather than shelling out to the
//! `ffmpeg` binary. The native libraries are pinned by the Nix dev shell so the
//! link is reproducible across machines and CI.

/// Initialize the FFmpeg-backed media subsystem.
///
/// Registers FFmpeg's demuxers, decoders, and filters. Call once at process
/// startup before any media I/O.
pub fn init() -> Result<(), crate::Error> {
    ffmpeg_next::init()?;
    tracing::debug!("ffmpeg media backend initialized");
    Ok(())
}
