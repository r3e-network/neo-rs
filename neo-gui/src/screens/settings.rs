//! Settings: the RPC endpoint and polling behaviour.

use std::time::Duration;

use egui::Ui;

use crate::app::NeoGuiApp;
use crate::sync::lock;
use crate::theme;
use crate::widgets;

const PRESETS: &[(&str, &str)] = &[
    ("Local MainNet", "http://127.0.0.1:10332"),
    ("Local TestNet", "http://127.0.0.1:20332"),
    ("Local private", "http://127.0.0.1:30332"),
];

pub fn ui(app: &mut NeoGuiApp, ui: &mut Ui) {
    ui.heading("Settings");
    ui.add_space(12.0);

    widgets::section(ui, "Connection");
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("RPC endpoint").color(theme::TEXT_MUTED));
            ui.add(
                egui::TextEdit::singleline(&mut app.url_edit)
                    .desired_width(360.0)
                    .hint_text("http://127.0.0.1:10332"),
            );
            if ui.button(egui::RichText::new("Connect").strong()).clicked() {
                let mut c = lock(&app.cfg, "PollerCfg");
                c.url = app.url_edit.trim().to_string();
                c.enabled = true;
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Presets").color(theme::TEXT_MUTED));
            for (name, url) in PRESETS {
                if ui.button(*name).clicked() {
                    app.url_edit = (*url).to_string();
                }
            }
        });

        ui.add_space(8.0);
        let mut enabled = lock(&app.cfg, "PollerCfg").enabled;
        if ui
            .checkbox(&mut enabled, "Poll the node automatically")
            .changed()
        {
            lock(&app.cfg, "PollerCfg").enabled = enabled;
        }

        let mut secs = lock(&app.cfg, "PollerCfg").interval.as_secs();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Poll interval (s)").color(theme::TEXT_MUTED));
            if ui.add(egui::Slider::new(&mut secs, 1..=30)).changed() {
                lock(&app.cfg, "PollerCfg").interval = Duration::from_secs(secs.max(1));
            }
        });
    });

    ui.add_space(14.0);
    widgets::section(ui, "About");
    widgets::card(ui, |ui| {
        ui.label(egui::RichText::new("Neo Node Manager").strong());
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new("A native Rust desktop manager for the neo-rs Neo N3 node.")
                .color(theme::TEXT_MUTED),
        );
    });
}
