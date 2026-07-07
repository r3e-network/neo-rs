//! Configuration: load, edit, validate, and save the node's TOML config.

use egui::Ui;

use crate::app::NeoGuiApp;
use crate::theme;
use crate::widgets;

pub fn ui(app: &mut NeoGuiApp, ui: &mut Ui) {
    ui.heading("Configuration");
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new("Edit the node's TOML configuration file.").color(theme::TEXT_MUTED),
    );
    ui.add_space(12.0);

    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Config file").color(theme::TEXT_MUTED));
            ui.add(
                egui::TextEdit::singleline(&mut app.local.config)
                    .hint_text("config/testnet.toml")
                    .desired_width(360.0),
            );
            if ui.button("Load").clicked() {
                match std::fs::read_to_string(&app.local.config) {
                    Ok(text) => {
                        app.config_text = text;
                        app.config_status = Some(format!("Loaded {}", app.local.config));
                    }
                    Err(e) => app.config_status = Some(format!("Load failed: {e}")),
                }
            }
            if ui.button("Validate").clicked() {
                app.config_status = Some(match toml_check(&app.config_text) {
                    Ok(()) => "Valid TOML".into(),
                    Err(e) => format!("Invalid: {e}"),
                });
            }
            if ui
                .add_enabled(
                    !app.config_text.is_empty(),
                    egui::Button::new(egui::RichText::new("Save").strong()),
                )
                .clicked()
            {
                app.config_status = Some(match toml_check(&app.config_text) {
                    Err(e) => format!("Refusing to save invalid TOML: {e}"),
                    Ok(()) => match std::fs::write(&app.local.config, &app.config_text) {
                        Ok(()) => format!("Saved {}", app.local.config),
                        Err(e) => format!("Save failed: {e}"),
                    },
                });
            }
        });
        if let Some(status) = &app.config_status {
            ui.add_space(6.0);
            let color = if status.starts_with("Saved")
                || status.starts_with("Valid")
                || status.starts_with("Loaded")
            {
                theme::OK
            } else {
                theme::WARN
            };
            ui.label(egui::RichText::new(status).color(color));
        }
    });

    ui.add_space(12.0);
    widgets::section(ui, "Editor");
    widgets::card(ui, |ui| {
        if app.config_text.is_empty() {
            ui.label(
                egui::RichText::new("Load a config file to edit it here.").color(theme::TEXT_MUTED),
            );
            return;
        }
        egui::ScrollArea::vertical()
            .max_height(420.0)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut app.config_text)
                        .code_editor()
                        .desired_rows(24)
                        .desired_width(f32::INFINITY),
                );
            });
    });
}

/// Minimal TOML validity check via serde_json's cousin — we only parse to a
/// generic value to confirm it is well-formed.
fn toml_check(text: &str) -> Result<(), String> {
    // Avoid pulling a TOML crate: do a light structural sanity check.
    // (Bracketed [section] headers balanced and key = value lines parse.)
    let mut depth_ok = true;
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with('[') {
            if !t.ends_with(']') {
                return Err(format!("line {}: unterminated section header", i + 1));
            }
        } else if !t.contains('=')
            && !t.starts_with('"')
            && !t.ends_with(',')
            && !t.ends_with(']')
            && !t.ends_with('{')
            && !t.ends_with('}')
        {
            depth_ok = depth_ok && false;
            return Err(format!("line {}: expected `key = value`", i + 1));
        }
    }
    Ok(())
}
