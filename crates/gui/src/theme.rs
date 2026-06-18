//! Visual theme for the desktop shell.
//!
//! A cohesive dark palette — cool near-black surfaces with a warm, cinematic
//! gold accent — installed once at startup via [`install`]. The palette is
//! exposed as named constants so callers paint with the same colors the global
//! [`egui::Style`] uses, and a handful of builders ([`card_frame`],
//! [`primary_button`], [`pill`], …) keep the widget look consistent across the
//! app. Swap the `ACCENT*` constants to retheme the whole shell.

use std::collections::BTreeMap;

use eframe::egui::{
    self, Color32, CornerRadius, FontId, Margin, RichText, Stroke, StrokeKind, TextStyle,
};

// Surfaces — cool near-black with a faint blue cast, layered darkest to lightest.
/// Central canvas behind the footage list.
pub const BG_APP: Color32 = Color32::from_rgb(20, 22, 27);
/// Toolbar and side-panel chrome.
pub const BG_PANEL: Color32 = Color32::from_rgb(27, 30, 37);
/// Cards, buttons, and inputs at rest.
pub const BG_CARD: Color32 = Color32::from_rgb(34, 38, 47);
/// Cards and buttons while hovered.
pub const BG_CARD_HOVER: Color32 = Color32::from_rgb(43, 48, 59);
/// Sunken wells such as the progress-bar track.
pub const BG_SUNKEN: Color32 = Color32::from_rgb(15, 16, 20);

/// Hairline borders and separators.
pub const STROKE_SOFT: Color32 = Color32::from_rgb(46, 51, 62);
/// Borders on hovered or emphasized surfaces.
pub const STROKE_STRONG: Color32 = Color32::from_rgb(64, 71, 85);

/// Headings and primary emphasis text.
pub const TEXT_STRONG: Color32 = Color32::from_rgb(242, 244, 248);
/// Body text.
pub const TEXT: Color32 = Color32::from_rgb(205, 210, 219);
/// Secondary text.
pub const TEXT_DIM: Color32 = Color32::from_rgb(143, 150, 163);
/// Faint metadata and disabled hints.
pub const TEXT_FAINT: Color32 = Color32::from_rgb(101, 108, 122);

// Accent — warm cinematic gold, used sparingly for the primary action and progress.
/// Primary accent.
pub const ACCENT: Color32 = Color32::from_rgb(232, 178, 58);
/// Text drawn on top of an [`ACCENT`] fill.
pub const ON_ACCENT: Color32 = Color32::from_rgb(28, 21, 6);

/// Status color for a successful probe.
pub const OK: Color32 = Color32::from_rgb(108, 196, 142);
/// Status color for an in-flight probe.
pub const BUSY: Color32 = Color32::from_rgb(118, 168, 220);
/// Status color for a failed probe.
pub const BAD: Color32 = Color32::from_rgb(230, 116, 124);

const SHADOW: egui::Shadow = egui::Shadow {
    offset: [0, 8],
    blur: 20,
    spread: 0,
    color: Color32::from_black_alpha(110),
};

/// Install the Hollywood theme into the egui context. Call once at startup.
pub fn install(ctx: &egui::Context) {
    ctx.global_style_mut(|style| {
        style.text_styles = text_styles();
        tune_spacing(&mut style.spacing);
        style.visuals = visuals();
        style.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    });
}

/// Frame for the top toolbar.
pub fn toolbar_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_PANEL)
        .inner_margin(Margin::symmetric(18, 12))
}

/// Frame for the right-hand export panel.
pub fn side_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_PANEL)
        .inner_margin(Margin::symmetric(18, 16))
}

/// Frame for the central footage area.
pub fn central_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_APP)
        .inner_margin(Margin::symmetric(22, 16))
}

/// Frame for a single footage card.
pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0, STROKE_SOFT))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 11))
}

/// The gold call-to-action button.
pub fn primary_button(label: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label).color(ON_ACCENT).strong())
        .fill(ACCENT)
        .corner_radius(CornerRadius::same(8))
        .min_size(egui::vec2(0.0, 30.0))
}

/// A neutral, framed secondary button matching the primary button's size.
pub fn secondary_button(label: &str) -> egui::Button<'static> {
    egui::Button::new(label)
        .corner_radius(CornerRadius::same(8))
        .min_size(egui::vec2(0.0, 30.0))
}

/// A large section heading.
pub fn section_header(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(18.0)
            .color(TEXT_STRONG)
            .extra_letter_spacing(0.3),
    );
}

/// A small uppercase label that introduces a group of controls.
pub fn overline(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(10.5)
            .color(TEXT_FAINT)
            .extra_letter_spacing(1.4),
    );
}

/// A small rounded status badge tinted with `color`.
pub fn pill(ui: &mut egui::Ui, label: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.16))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
        .corner_radius(CornerRadius::same(7))
        .inner_margin(Margin::symmetric(9, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(11.0).color(color));
        });
}

/// The gold app mark: a rounded chip with a play glyph. `size` is its side
/// length; `corner` its corner radius in points.
pub fn logo_mark(ui: &mut egui::Ui, size: f32, corner: u8) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(corner), ACCENT);
    let center = rect.center();
    let half_h = size * 0.22;
    let half_w = size * 0.20;
    let glyph = vec![
        egui::pos2(center.x - half_w, center.y - half_h),
        egui::pos2(center.x - half_w, center.y + half_h),
        egui::pos2(center.x + half_w * 1.4, center.y),
    ];
    painter.add(egui::Shape::convex_polygon(glyph, ON_ACCENT, Stroke::NONE));
}

/// A large outlined "viewport" mark with a gold play glyph, for the empty state.
pub fn hero_mark(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        CornerRadius::same(18),
        BG_CARD,
        Stroke::new(1.5, STROKE_STRONG),
        StrokeKind::Inside,
    );
    let center = rect.center();
    let half_h = size * 0.20;
    let half_w = size * 0.17;
    let glyph = vec![
        egui::pos2(center.x - half_w, center.y - half_h),
        egui::pos2(center.x - half_w, center.y + half_h),
        egui::pos2(center.x + half_w * 1.5, center.y),
    ];
    painter.add(egui::Shape::convex_polygon(glyph, ACCENT, Stroke::NONE));
}

fn text_styles() -> BTreeMap<TextStyle, FontId> {
    [
        (TextStyle::Small, FontId::proportional(11.0)),
        (TextStyle::Body, FontId::proportional(14.0)),
        (TextStyle::Button, FontId::proportional(14.0)),
        (TextStyle::Monospace, FontId::monospace(12.5)),
        (TextStyle::Heading, FontId::proportional(19.0)),
    ]
    .into()
}

fn tune_spacing(spacing: &mut egui::Spacing) {
    spacing.item_spacing = egui::vec2(10.0, 8.0);
    spacing.button_padding = egui::vec2(12.0, 7.0);
    spacing.interact_size.y = 26.0;
    spacing.icon_width = 18.0;
    spacing.icon_width_inner = 10.0;
    spacing.icon_spacing = 8.0;
    spacing.menu_margin = Margin::same(8);
    spacing.scroll = egui::style::ScrollStyle::floating();
}

fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG_PANEL;
    visuals.window_fill = BG_PANEL;
    visuals.extreme_bg_color = BG_SUNKEN;
    visuals.faint_bg_color = BG_CARD;
    visuals.code_bg_color = BG_SUNKEN;
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = ACCENT;
    visuals.error_fg_color = BAD;
    visuals.weak_text_color = Some(TEXT_DIM);
    visuals.window_stroke = Stroke::new(1.0, STROKE_SOFT);
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.window_shadow = SHADOW;
    visuals.popup_shadow = SHADOW;
    visuals.selection = egui::style::Selection {
        bg_fill: ACCENT.gamma_multiply(0.22),
        stroke: Stroke::new(1.0, ACCENT),
    };
    visuals.widgets = widgets();
    visuals
}

fn widgets() -> egui::style::Widgets {
    let mut widgets = egui::style::Widgets::dark();

    widgets.noninteractive.bg_fill = BG_PANEL;
    widgets.noninteractive.weak_bg_fill = BG_PANEL;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, STROKE_SOFT);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.noninteractive.corner_radius = CornerRadius::same(8);

    widgets.inactive.bg_fill = BG_CARD;
    widgets.inactive.weak_bg_fill = BG_CARD;
    widgets.inactive.bg_stroke = Stroke::new(1.0, STROKE_SOFT);
    widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    widgets.inactive.corner_radius = CornerRadius::same(8);

    widgets.hovered.bg_fill = BG_CARD_HOVER;
    widgets.hovered.weak_bg_fill = BG_CARD_HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0, STROKE_STRONG);
    widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    widgets.hovered.corner_radius = CornerRadius::same(8);

    widgets.active.bg_fill = BG_CARD_HOVER;
    widgets.active.weak_bg_fill = BG_CARD_HOVER;
    widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    widgets.active.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    widgets.active.corner_radius = CornerRadius::same(8);

    widgets.open = widgets.inactive;
    widgets
}
