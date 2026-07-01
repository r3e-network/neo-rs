//! Node: control a locally-spawned `neo-node` process and tail its logs.

use egui::Ui;

use crate::app::NeoGuiApp;
use crate::theme;
use crate::widgets;

pub fn ui(app: &mut NeoGuiApp, ui: &mut Ui) {
    ui.heading("Node");
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new("Run and supervise a local neo-node process.")
            .color(theme::TEXT_MUTED),
    );
    ui.add_space(12.0);

    let running = app.local.is_running();

    widgets::card(ui, |ui| {
        egui::Grid::new("node_paths")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Binary").color(theme::TEXT_MUTED));
                ui.add(
                    egui::TextEdit::singleline(&mut app.local.binary)
                        .hint_text("./target/release/neo-node")
                        .desired_width(440.0),
                );
                ui.end_row();
                ui.label(egui::RichText::new("Config").color(theme::TEXT_MUTED));
                ui.add(
                    egui::TextEdit::singleline(&mut app.local.config)
                        .hint_text("config/testnet.toml")
                        .desired_width(440.0),
                );
                ui.end_row();
            });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            crate::widgets::status_pill(ui, running);
            ui.add_space(10.0);
            if running {
                if ui.button(egui::RichText::new("■ Stop").strong()).clicked() {
                    app.local.stop();
                }
            } else {
                let can_start = !app.local.binary.trim().is_empty();
                if ui
                    .add_enabled(can_start, egui::Button::new(egui::RichText::new("▶ Start").strong()))
                    .clicked()
                {
                    if let Err(e) = app.local.start() {
                        if let Ok(mut o) = app.rpc_out.lock() {
                            *o = Some(format!("failed to start node: {e}"));
                        }
                    }
                }
            }
        });
    });

    ui.add_space(14.0);
    widgets::section(ui, "Logs");
    let lines = app.local.log_lines();
    widgets::card(ui, |ui| {
        if lines.is_empty() {
            ui.label(egui::RichText::new("No output yet.").color(theme::TEXT_MUTED));
            return;
        }
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for line in lines.iter().rev().take(500).rev() {
                    ui.label(egui::RichText::new(line).monospace().size(12.0));
                }
            });
    });
}
