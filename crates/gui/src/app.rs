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
use crate::theme;

/// Initial window size, in logical points: room for the footage list plus the
/// export side panel. The window is freely resizable; this is only the opening
/// size.
const DEFAULT_WINDOW_SIZE: [f32; 2] = [1100.0, 720.0];

/// Smallest the window may shrink to before the toolbar actions and the export
/// panel start to crowd.
const MIN_WINDOW_SIZE: [f32; 2] = [720.0, 480.0];

/// Launch the Hollywood desktop shell.
///
/// # Errors
///
/// Returns [`eframe::Error`] if the windowing backend fails to start.
pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(DEFAULT_WINDOW_SIZE)
            .with_min_inner_size(MIN_WINDOW_SIZE),
        ..Default::default()
    };

    eframe::run_native(
        "Hollywood",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(HollywoodApp::new()))
        }),
    )
}

struct HollywoodApp {
    footage: Vec<FootageEntry>,
    export: ExportSelection,
    progress: f32,
    /// Accumulated frame time, in seconds, driving the burning loading indicator.
    anim: f32,
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
            anim: 0.0,
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

    /// What the loading indicator should show: an ember sweep while footage is
    /// probing, the pipeline's progress once it runs, otherwise nothing.
    fn burn(&self) -> theme::Burn {
        if self.probe_rx.is_some()
            || self
                .footage
                .iter()
                .any(|f| matches!(f.outcome(), ProbeOutcome::Pending))
        {
            theme::Burn::Indeterminate
        } else if self.progress > 0.0 {
            theme::Burn::Fraction(self.progress)
        } else {
            theme::Burn::Idle
        }
    }
}

impl eframe::App for HollywoodApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_background(ctx);
        if matches!(
            self.burn(),
            theme::Burn::Indeterminate | theme::Burn::Fraction(_)
        ) {
            self.anim += ctx.input(|i| i.stable_dt);
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("toolbar")
            .resizable(false)
            .frame(theme::toolbar_frame())
            .show_inside(ui, |ui| self.toolbar(ui));

        egui::Panel::right("export")
            .resizable(true)
            .default_size(300.0)
            .frame(theme::side_frame())
            .show_inside(ui, |ui| self.export_panel(ui));

        egui::CentralPanel::default()
            .frame(theme::central_frame())
            .show_inside(ui, |ui| self.footage_panel(ui));
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::BG_APP.to_normalized_gamma_f32()
    }
}

impl HollywoodApp {
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            theme::mark(ui, 30.0, 8);
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Hollywood")
                    .size(22.0)
                    .color(theme::TEXT_STRONG)
                    .extra_letter_spacing(0.5),
            );

            let busy = self.picker_rx.is_some() || self.probe_rx.is_some();
            let ready = self.can_process();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(ready, |ui| {
                    // Pipeline orchestration lands in hollywood-pipeline.
                    ui.add(theme::primary_button("Process"))
                        .on_hover_text("Pipeline coming soon");
                });
                ui.add_space(8.0);
                ui.add_enabled_ui(!busy, |ui| {
                    if ui.add(theme::secondary_button("Add footage")).clicked() {
                        self.begin_pick_footage();
                    }
                });
            });
        });
    }

    fn export_panel(&mut self, ui: &mut egui::Ui) {
        theme::section_header(ui, "Export");
        ui.add_space(14.0);

        theme::overline(ui, "FORMATS");
        ui.add_space(8.0);
        for target in ExportTarget::all() {
            self.export_target_row(ui, target);
        }

        ui.add_space(20.0);
        theme::overline(ui, "PROGRESS");
        ui.add_space(8.0);
        let burn = self.burn();
        theme::fire_bar(ui, &burn, self.anim);
        ui.add_space(8.0);
        let caption = match burn {
            theme::Burn::Indeterminate => "Probing footage…".to_owned(),
            theme::Burn::Fraction(fraction) => format!("{:.0}%", fraction * 100.0),
            theme::Burn::Idle => "Waiting for pipeline…".to_owned(),
        };
        ui.label(egui::RichText::new(caption).color(theme::TEXT_DIM));
    }

    fn export_target_row(&mut self, ui: &mut egui::Ui, target: ExportTarget) {
        let selected = self.export.contains(target);
        let available = target.is_available();
        let mut chip = egui::Button::new(target.label())
            .selected(selected)
            .wrap()
            .corner_radius(egui::CornerRadius::same(8))
            .min_size(egui::vec2(ui.available_width(), 32.0));
        if selected {
            chip = chip.stroke(egui::Stroke::new(1.0, theme::TEAL));
        }
        let response = ui.add_enabled(available, chip);
        if available && response.clicked() {
            self.export.set(target, !selected);
        }
        if !available {
            response.on_disabled_hover_text("Not implemented yet");
        }
        ui.add_space(6.0);
    }

    fn footage_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            theme::section_header(ui, "Footage");
            if !self.footage.is_empty() {
                ui.add_space(4.0);
                theme::pill(ui, &self.footage.len().to_string(), theme::TEAL);
            }
        });
        ui.add_space(12.0);

        if self.footage.is_empty() {
            self.empty_state(ui);
            return;
        }

        egui::ScrollArea::vertical()
            .auto_shrink(egui::Vec2b::new(false, false))
            .show(ui, |ui| {
                for entry in &self.footage {
                    Self::footage_card(ui, entry);
                }
            });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        // Center the hero block vertically in the remaining well (~210px tall).
        let top = ((ui.available_height() - 210.0) * 0.5).max(24.0);
        ui.add_space(top);
        ui.vertical_centered(|ui| {
            theme::mark(ui, 88.0, 22);
            ui.add_space(18.0);
            ui.label(
                egui::RichText::new("No footage yet")
                    .size(20.0)
                    .color(theme::TEXT_STRONG),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Add video or audio files to start a rough cut.")
                    .color(theme::TEXT_DIM),
            );
            ui.add_space(20.0);
            if ui.add(theme::primary_button("Add footage")).clicked() {
                self.begin_pick_footage();
            }
        });
    }

    fn footage_card(ui: &mut egui::Ui, entry: &FootageEntry) {
        theme::card_frame().show(ui, |ui| {
            let (status, color) = outcome_status(entry.outcome());
            // Pin the status pill to the top-right, then let the text column
            // fill (and wrap within) the remaining width.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                theme::pill(ui, status, color);
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(entry.label())
                            .size(15.0)
                            .color(theme::TEXT_STRONG),
                    );
                    ui.add_space(3.0);
                    ui.label(egui::RichText::new(entry.outcome().summary()).color(theme::TEXT_DIM));
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(entry.source().to_string())
                            .small()
                            .color(theme::TEXT_FAINT),
                    );
                });
            });
        });
    }
}

fn outcome_status(outcome: &ProbeOutcome) -> (&'static str, egui::Color32) {
    match outcome {
        ProbeOutcome::Pending => ("Probing", theme::BUSY),
        ProbeOutcome::Ready(_) => ("Ready", theme::OK),
        ProbeOutcome::Failed(_) => ("Failed", theme::BAD),
    }
}
