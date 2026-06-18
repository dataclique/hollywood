//! Visual theme for the desktop shell.
//!
//! The design is derived from the subject, not from fashion: Hollywood's own
//! color grade. Shadows are graded toward teal and near-black; the accents are
//! the cinematic **orange-and-teal** complementary pair — warm orange for
//! action and fire, cool teal for selection and information. The app mark is a
//! sunset over the Hollywood Hills, and the loading indicator burns like the
//! hills do every summer ([`fire_bar`]).
//!
//! The theme is always dark (a creative tool, like Resolve or Premiere):
//! [`install`] pins egui to dark and applies the palette to every style so the
//! OS light/dark toggle can never leave it half-themed.

use std::collections::BTreeMap;

use eframe::egui::{
    self, Color32, CornerRadius, FontId, Margin, Rect, RichText, Stroke, TextStyle,
};

// Surfaces — graded blacks with a cool teal cast (shadows pushed teal).
/// Central canvas behind the footage list.
pub const BG_APP: Color32 = Color32::from_rgb(10, 13, 14);
/// Toolbar and side-panel chrome.
pub const BG_PANEL: Color32 = Color32::from_rgb(14, 19, 22);
/// Cards, buttons, and inputs at rest.
pub const BG_CARD: Color32 = Color32::from_rgb(20, 27, 31);
/// Cards and buttons while hovered.
pub const BG_CARD_HOVER: Color32 = Color32::from_rgb(27, 36, 42);
/// Sunken wells such as the loading-bar track.
pub const BG_SUNKEN: Color32 = Color32::from_rgb(6, 9, 10);

/// Hairline borders and separators.
pub const STROKE_SOFT: Color32 = Color32::from_rgb(28, 36, 42);
/// Borders on hovered or emphasized surfaces.
pub const STROKE_STRONG: Color32 = Color32::from_rgb(43, 55, 62);

/// Headings and primary emphasis text.
pub const TEXT_STRONG: Color32 = Color32::from_rgb(242, 246, 247);
/// Body text.
pub const TEXT: Color32 = Color32::from_rgb(191, 203, 208);
/// Secondary text.
pub const TEXT_DIM: Color32 = Color32::from_rgb(124, 139, 146);
/// Faint metadata.
pub const TEXT_FAINT: Color32 = Color32::from_rgb(78, 87, 93);

// The grade: warm highlights + cool shadows.
/// Warm accent — the primary action and fire.
pub const ORANGE: Color32 = Color32::from_rgb(255, 138, 61);
/// Text drawn on top of an [`ORANGE`] fill.
pub const ON_ORANGE: Color32 = Color32::from_rgb(24, 16, 10);
/// Cool accent — selection, focus, and information.
pub const TEAL: Color32 = Color32::from_rgb(52, 195, 212);

/// Status color for a successful probe.
pub const OK: Color32 = TEAL;
/// Status color for an in-flight probe.
pub const BUSY: Color32 = ORANGE;
/// Status color for a failed probe.
pub const BAD: Color32 = Color32::from_rgb(255, 82, 71);

const SHADOW: egui::Shadow = egui::Shadow {
    offset: [0, 10],
    blur: 24,
    spread: 0,
    color: Color32::from_black_alpha(140),
};

// Logo: the HOLLYWOOD sign on its hillside at dusk.
const SKY: Color32 = Color32::from_rgb(20, 31, 38);
const HILL: Color32 = Color32::from_rgb(67, 58, 47);
const SIGN: Color32 = Color32::from_rgb(236, 232, 223);

// Fire: the burning loading indicator.
const FIRE_BASE: Color32 = Color32::from_rgb(150, 28, 18);
const FIRE_MID: Color32 = Color32::from_rgb(255, 106, 30);
const FIRE_TIP: Color32 = Color32::from_rgb(255, 216, 110);

/// What the loading indicator should show.
pub enum Burn {
    /// Nothing is running.
    Idle,
    /// Work of unknown duration (e.g. probing footage).
    Indeterminate,
    /// Determinate progress in `0.0..=1.0`.
    Fraction(f32),
}

/// Install the Hollywood theme. Call once at startup.
///
/// Pins egui to its dark theme and applies the palette to both the dark and
/// light styles, so following the OS appearance can never leave the UI
/// half-themed.
pub fn install(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
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
        .inner_margin(Margin::symmetric(20, 14))
}

/// Frame for the right-hand export panel.
pub fn side_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_PANEL)
        .inner_margin(Margin::symmetric(20, 18))
}

/// Frame for the central footage area.
pub fn central_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_APP)
        .inner_margin(Margin::symmetric(24, 18))
}

/// Frame for a single footage card.
pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0, STROKE_SOFT))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(16, 13))
}

/// The warm call-to-action button.
pub fn primary_button(label: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label).color(ON_ORANGE))
        .fill(ORANGE)
        .corner_radius(CornerRadius::same(9))
        .min_size(egui::vec2(0.0, 32.0))
}

/// A neutral, framed secondary button matching the primary button's size.
pub fn secondary_button(label: &str) -> egui::Button<'static> {
    egui::Button::new(label)
        .corner_radius(CornerRadius::same(9))
        .min_size(egui::vec2(0.0, 32.0))
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

/// The app mark: the HOLLYWOOD sign on its hillside at dusk. `size` is the
/// side length, `corner` the corner radius in points.
pub fn mark(ui: &mut egui::Ui, size: f32, corner: u8) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    paint_mark(&ui.painter_at(rect), rect, corner);
}

/// The loading indicator: a burning fuse. Drive `phase` from accumulated frame
/// time so the tip flickers and the indeterminate ember sweeps.
pub fn fire_bar(ui: &mut egui::Ui, burn: &Burn, phase: f32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 12.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(6), BG_SUNKEN);

    match burn {
        Burn::Idle => {}
        Burn::Indeterminate => {
            let seg = rect.width() * 0.30;
            let travel = rect.width() + seg;
            let start = (phase * 0.5).fract().mul_add(travel, rect.left()) - seg;
            let left = start.max(rect.left());
            let right = (start + seg).min(rect.right());
            if right > left {
                let fill = Rect::from_min_max(
                    egui::pos2(left, rect.top()),
                    egui::pos2(right, rect.bottom()),
                );
                paint_fire(&painter, fill, phase);
            }
        }
        Burn::Fraction(fraction) => {
            let width = rect.width() * fraction.clamp(0.0, 1.0);
            if width > 1.0 {
                let fill =
                    Rect::from_min_max(rect.min, egui::pos2(rect.left() + width, rect.bottom()));
                paint_fire(&painter, fill, phase);
            }
        }
    }
}

fn paint_mark(painter: &egui::Painter, rect: Rect, corner: u8) {
    let w = rect.width();
    let h = rect.height();
    let left = rect.left();
    let top = rect.top();
    painter.rect_filled(rect, CornerRadius::same(corner), SKY);

    let horizon = h.mul_add(0.62, top);

    // A faint golden-hour band along the horizon — dusk, not a literal sun.
    let glow_above = h * 0.07;
    let glow_below = h * 0.02;
    let glow = Rect::from_min_max(
        egui::pos2(left, horizon - glow_above),
        egui::pos2(rect.right(), horizon + glow_below),
    );
    painter.rect_filled(glow, CornerRadius::ZERO, ORANGE.gamma_multiply(0.20));

    // The hillside: flat ground plus a single gently rounded crest (the cap of
    // a large circle), so it reads as a hill, not a jagged peak.
    let ground = Rect::from_min_max(egui::pos2(left, horizon), rect.max);
    painter.rect_filled(
        ground,
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: corner,
            se: corner,
        },
        HILL,
    );
    let radius = w * 0.64;
    let peak = h * 0.20;
    let center_x = w.mul_add(0.50, left);
    let center_y = radius + horizon - peak;
    painter.circle_filled(egui::pos2(center_x, center_y), radius, HILL);

    // The HOLLYWOOD sign: thin white letters standing on the crest, each one
    // sitting on the curve of the hillside.
    let bar_w = w * 0.045;
    let bar_half = bar_w * 0.5;
    let gap = w * 0.03;
    let pitch = bar_w + gap;
    let span = pitch.mul_add(8.0, -gap);
    let half_span = span * 0.5;
    let bar_h = h * 0.15;
    let radius_sq = radius * radius;
    let mut x = center_x - half_span;
    // Eight uprights stand in for the HOLLYWOOD letters (matches the 8.0 span).
    for _ in 0..8 {
        let dx = x + bar_half - center_x;
        let surface = center_y - dx.mul_add(-dx, radius_sq).max(0.0).sqrt();
        let bar = Rect::from_min_max(
            egui::pos2(x, surface - bar_h),
            egui::pos2(x + bar_w, surface + glow_below),
        );
        painter.rect_filled(bar, CornerRadius::ZERO, SIGN);
        x += pitch;
    }
}

fn paint_fire(painter: &egui::Painter, fill: Rect, phase: f32) {
    painter.rect_filled(fill, CornerRadius::same(6), FIRE_MID);

    // A deep-red ember root, rounded only on the leading-out (left) end.
    if fill.width() > 16.0 {
        let base_width = (fill.width() * 0.4).min(fill.width() - 6.0);
        let base = Rect::from_min_max(
            fill.min,
            egui::pos2(fill.left() + base_width, fill.bottom()),
        );
        painter.rect_filled(
            base,
            CornerRadius {
                nw: 6,
                ne: 0,
                sw: 6,
                se: 0,
            },
            FIRE_BASE,
        );
    }

    // A hot, flickering tip.
    let flicker = 0.22f32.mul_add((phase * 12.0).sin(), 0.78).clamp(0.4, 1.0);
    painter.circle_filled(
        egui::pos2(fill.right(), fill.center().y),
        fill.height() * 0.66,
        FIRE_TIP.gamma_multiply(flicker),
    );
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
    spacing.button_padding = egui::vec2(14.0, 8.0);
    spacing.interact_size.y = 28.0;
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
    visuals.hyperlink_color = TEAL;
    visuals.warn_fg_color = ORANGE;
    visuals.error_fg_color = BAD;
    visuals.weak_text_color = Some(TEXT_DIM);
    visuals.window_stroke = Stroke::new(1.0, STROKE_SOFT);
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(12);
    visuals.window_shadow = SHADOW;
    visuals.popup_shadow = SHADOW;
    visuals.selection = egui::style::Selection {
        bg_fill: TEAL.gamma_multiply(0.20),
        stroke: Stroke::new(1.0, TEAL),
    };
    visuals.widgets = widgets();
    visuals
}

fn widgets() -> egui::style::Widgets {
    let mut widgets = egui::style::Widgets::dark();
    let radius = CornerRadius::same(10);

    widgets.noninteractive.bg_fill = BG_PANEL;
    widgets.noninteractive.weak_bg_fill = BG_PANEL;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, STROKE_SOFT);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.noninteractive.corner_radius = radius;

    widgets.inactive.bg_fill = BG_CARD;
    widgets.inactive.weak_bg_fill = BG_CARD;
    widgets.inactive.bg_stroke = Stroke::new(1.0, STROKE_SOFT);
    widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    widgets.inactive.corner_radius = radius;

    widgets.hovered.bg_fill = BG_CARD_HOVER;
    widgets.hovered.weak_bg_fill = BG_CARD_HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0, STROKE_STRONG);
    widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    widgets.hovered.corner_radius = radius;

    widgets.active.bg_fill = BG_CARD_HOVER;
    widgets.active.weak_bg_fill = BG_CARD_HOVER;
    widgets.active.bg_stroke = Stroke::new(1.0, TEAL);
    widgets.active.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    widgets.active.corner_radius = radius;

    widgets.open = widgets.inactive;
    widgets
}
