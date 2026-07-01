//! A polished dark theme for the manager, built around Neo's green accent.

use egui::{Color32, Context, FontFamily, FontId, Rounding, Stroke, TextStyle, Visuals};

/// Neo brand green.
pub const ACCENT: Color32 = Color32::from_rgb(0, 226, 150);
/// A dimmer accent for hover/secondary use.
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0, 168, 112);
/// Panel / sidebar background.
pub const BG_PANEL: Color32 = Color32::from_rgb(20, 24, 28);
/// Main content background.
pub const BG_CONTENT: Color32 = Color32::from_rgb(26, 31, 36);
/// Card / raised surface background.
pub const BG_CARD: Color32 = Color32::from_rgb(33, 39, 45);
/// Subtle border colour.
pub const BORDER: Color32 = Color32::from_rgb(48, 56, 64);
/// Primary text.
pub const TEXT: Color32 = Color32::from_rgb(226, 232, 238);
/// Muted / secondary text.
pub const TEXT_MUTED: Color32 = Color32::from_rgb(140, 152, 164);
/// Success / online.
pub const OK: Color32 = Color32::from_rgb(0, 226, 150);
/// Warning.
pub const WARN: Color32 = Color32::from_rgb(240, 180, 60);
/// Error / offline.
pub const ERR: Color32 = Color32::from_rgb(240, 90, 90);

/// Install the theme into the egui context.
pub fn install(ctx: &Context) {
    let mut visuals = Visuals::dark();

    visuals.panel_fill = BG_CONTENT;
    visuals.window_fill = BG_PANEL;
    visuals.extreme_bg_color = Color32::from_rgb(15, 18, 21);
    visuals.faint_bg_color = BG_CARD;
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT_DIM.linear_multiply(0.5);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.window_rounding = Rounding::same(10.0);
    visuals.window_stroke = Stroke::new(1.0, BORDER);

    let rounding = Rounding::same(8.0);
    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.weak_bg_fill = BG_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.noninteractive.rounding = rounding;

    visuals.widgets.inactive.bg_fill = BG_CARD;
    visuals.widgets.inactive.weak_bg_fill = BG_CARD;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_MUTED);
    visuals.widgets.inactive.rounding = rounding;

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(42, 49, 56);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(42, 49, 56);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_DIM);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.rounding = rounding;

    visuals.widgets.active.bg_fill = ACCENT_DIM;
    visuals.widgets.active.weak_bg_fill = ACCENT_DIM;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::BLACK);
    visuals.widgets.active.rounding = rounding;

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.window_margin = egui::Margin::same(12.0);
    style.text_styles = [
        (TextStyle::Heading, FontId::new(22.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
    ]
    .into();
    ctx.set_style(style);
}
