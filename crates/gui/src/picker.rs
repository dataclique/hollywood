//! Native file dialogs without blocking the `egui` event loop.
//!
//! `rfd`'s synchronous API runs on a worker thread; the UI polls the channel
//! each frame (ADR 0005).

use std::path::PathBuf;
use std::sync::mpsc;

/// Result of a footage file-picker dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PickerResult {
    /// The user chose one or more files.
    Files(Vec<PathBuf>),
    /// The dialog closed without a selection.
    Cancelled,
}

/// Start a multi-select media file dialog. Poll [`mpsc::Receiver::try_recv`] from
/// the UI thread.
pub(crate) fn open_footage_picker() -> mpsc::Receiver<PickerResult> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let paths = rfd::FileDialog::new()
            .set_title("Add footage")
            .add_filter(
                "media",
                &[
                    "mp4", "mov", "mkv", "m4v", "avi", "webm", "wav", "mp3", "aac", "flac", "m4a",
                ],
            )
            .pick_files();
        let result = match paths {
            Some(files) if !files.is_empty() => PickerResult::Files(files),
            _ => PickerResult::Cancelled,
        };
        let _ = tx.send(result);
    });
    rx
}
