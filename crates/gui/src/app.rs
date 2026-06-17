//! Main `eframe` application state and views.

use std::path::PathBuf;
use std::sync::mpsc;

use eframe::egui;
use hollywood_ffmpeg::{FfmpegProbe, MediaProbe};
use hollywood_timeline::MediaSource;
use tracing::warn;

use crate::export::{ExportSelection, ExportTarget};
use crate::footage::{FootageEntry, ProbeOutcome};
use crate::picker::{PickerResult, open_footage_picker};

/// Launch the Hollywood desktop shell.
///
/// # Errors
///
/// Returns [`eframe::Error`] if the windowing backend fails to start.
pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Hollywood",
        options,
        Box::new(|_cc| Ok(Box::new(HollywoodApp::new()))),
    )
}

struct HollywoodApp {
    footage: Vec<FootageEntry>,
    export: ExportSelection,
    progress: f32,
    picker_rx: Option<mpsc::Receiver<PickerResult>>,
    probe_rx: Option<mpsc::Receiver<ProbeBatch>>,
}

struct ProbeBatch {
    start: usize,
    entries: Vec<FootageEntry>,
}

impl HollywoodApp {
    fn new() -> Self {
        Self {
            footage: Vec::new(),
            export: ExportSelection::default_enabled(),
            progress: 0.0,
            picker_rx: None,
            probe_rx: None,
        }
    }

    fn poll_background(&mut self, ctx: &egui::Context) {
        let mut repaint = false;

        if let Some(rx) = self.picker_rx.as_ref() {
            match rx.try_recv() {
                Ok(PickerResult::Files(paths)) => {
                    self.picker_rx = None;
                    self.start_probe(paths);
                    repaint = true;
                }
                Ok(PickerResult::Cancelled) => {
                    self.picker_rx = None;
                    repaint = true;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.picker_rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        if let Some(rx) = self.probe_rx.as_ref() {
            match rx.try_recv() {
                Ok(batch) => {
                    self.probe_rx = None;
                    for (offset, entry) in batch.entries.into_iter().enumerate() {
                        let index = batch.start + offset;
                        if let Some(slot) = self.footage.get_mut(index) {
                            *slot = entry;
                        }
                    }
                    repaint = true;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.probe_rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        if repaint {
            ctx.request_repaint();
        }
    }

    fn start_probe(&mut self, paths: Vec<PathBuf>) {
        let start = self.footage.len();
        for path in &paths {
            self.footage
                .push(FootageEntry::pending(MediaSource::file(path)));
        }

        let (tx, rx) = mpsc::channel();
        self.probe_rx = Some(rx);
        std::thread::spawn(move || {
            let probe = FfmpegProbe;
            let entries: Vec<FootageEntry> = paths
                .into_iter()
                .map(|path| {
                    let source = MediaSource::file(&path);
                    let outcome = match probe.probe(&path) {
                        Ok(media) => ProbeOutcome::Ready(media),
                        Err(error) => {
                            warn!(path = %path.display(), %error, "probe failed");
                            ProbeOutcome::Failed(error.to_string())
                        }
                    };
                    FootageEntry::probed(source, outcome)
                })
                .collect();
            let _ = tx.send(ProbeBatch { start, entries });
        });
    }

    fn begin_pick_footage(&mut self) {
        if self.picker_rx.is_none() {
            self.picker_rx = Some(open_footage_picker());
        }
    }

    fn can_process(&self) -> bool {
        !self.footage.is_empty()
            && self.export.has_implemented_target()
            && self
                .footage
                .iter()
                .all(|f| !matches!(f.outcome(), ProbeOutcome::Pending))
    }
}

impl eframe::App for HollywoodApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_background(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("toolbar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Hollywood");
                ui.separator();
                let picking = self.picker_rx.is_some();
                let probing = self.probe_rx.is_some();
                if ui
                    .add_enabled(!picking && !probing, egui::Button::new("Add footage"))
                    .clicked()
                {
                    self.begin_pick_footage();
                }
                ui.separator();
                let process = ui.add_enabled(
                    self.can_process(),
                    egui::Button::new("Process (coming soon)"),
                );
                if process.clicked() {
                    // Pipeline orchestration lands in hollywood-pipeline.
                }
            });
        });

        egui::Panel::right("export")
            .resizable(true)
            .show_inside(ui, |ui| {
                ui.heading("Export");
                ui.separator();
                for target in ExportTarget::all() {
                    let mut on = self.export.contains(target);
                    let response = ui.add_enabled(
                        target.is_available(),
                        egui::Checkbox::new(&mut on, target.label()),
                    );
                    if target.is_available() {
                        self.export.set(target, on);
                    } else if response.clicked() {
                        ui.label("not implemented yet");
                    }
                }
                ui.separator();
                ui.label("Progress");
                ui.add(egui::ProgressBar::new(self.progress).show_percentage());
                if self.progress == 0.0 {
                    ui.label("Waiting for pipeline…");
                }
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.heading("Footage");
            ui.separator();

            if self.footage.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.label("Add one or more video or audio files to get started.");
                    if ui.button("Add footage").clicked() {
                        self.begin_pick_footage();
                    }
                });
                return;
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                for entry in &self.footage {
                    ui.group(|ui| {
                        ui.label(egui::RichText::new(entry.label()).strong());
                        ui.label(entry.outcome().summary());
                        ui.label(
                            egui::RichText::new(entry.source().to_string())
                                .small()
                                .weak(),
                        );
                    });
                    ui.add_space(4.0);
                }
            });
        });
    }
}
