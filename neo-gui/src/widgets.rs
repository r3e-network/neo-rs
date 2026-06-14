//! Small reusable UI building blocks for a consistent, professional look.

use egui::{Color32, Rounding, Stroke, Ui, Vec2};

use crate::theme;

/// A raised card container with padding and a subtle border.
pub fn card(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    egui::Frame::none()
        .fill(theme::BG_CARD)
        .rounding(Rounding::same(10.0))
        .stroke(Stroke::new(1.0, theme::BORDER))
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, add_contents);
}

/// A stat tile: a small muted label over a large value, with an accent dot.
pub fn stat_card(ui: &mut Ui, label: &str, value: impl Into<String>, accent: Color32) {
    let value = value.into();
    egui::Frame::none()
        .fill(theme::BG_CARD)
        .rounding(Rounding::same(10.0))
        .stroke(Stroke::new(1.0, theme::BORDER))
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, |ui| {
            ui.set_min_size(Vec2::new(180.0, 78.0));
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.0, accent);
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(label).color(theme::TEXT_MUTED).size(12.0));
                });
                ui.add_space(6.0);
                ui.label(egui::RichText::new(value).size(24.0).strong().color(theme::TEXT));
            });
        });
}

/// An online/offline pill.
pub fn status_pill(ui: &mut Ui, online: bool) {
    let (txt, color) = if online {
        ("● Online", theme::OK)
    } else {
        ("● Offline", theme::ERR)
    };
    egui::Frame::none()
        .fill(color.linear_multiply(0.16))
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::symmetric(10.0, 4.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(txt).color(color).size(12.5).strong());
        });
}

/// A section heading with a muted rule beneath it.
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(title).size(16.0).strong().color(theme::TEXT));
    ui.add_space(2.0);
    let rect = ui.available_rect_before_wrap();
    let y = ui.cursor().top();
    ui.painter().hline(
        rect.left()..=rect.right(),
        y,
        Stroke::new(1.0, theme::BORDER),
    );
    ui.add_space(8.0);
}

/// A labelled key/value row (muted key, mono value).
pub fn kv(ui: &mut Ui, key: &str, value: impl Into<String>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(key).color(theme::TEXT_MUTED).size(13.0));
        ui.add_space(6.0);
        ui.label(egui::RichText::new(value.into()).monospace().color(theme::TEXT));
    });
}
