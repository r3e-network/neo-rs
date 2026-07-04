//! Integrations: third-party monitoring, logging, uptime, alerting, and error
//! services — configure endpoints/keys and send a test ping.

use std::sync::{Arc, Mutex};

use egui::Ui;

use crate::app::NeoGuiApp;
use crate::theme;
use crate::widgets;

pub fn ui(app: &mut NeoGuiApp, ui: &mut Ui) {
    ui.heading("Integrations");
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(
            "Connect metrics, logging, uptime, alerting, and error-tracking services.",
        )
        .color(theme::TEXT_MUTED),
    );
    ui.add_space(12.0);

    let kinds = ["Metrics", "Logging", "Uptime", "Alerting", "Errors"];
    let ctx = ui.ctx().clone();
    let status_sink = Arc::clone(&app.integration_status);

    for kind in kinds {
        let any = app.integrations.iter().any(|i| i.kind == kind);
        if !any {
            continue;
        }
        widgets::section(ui, kind);
        for integ in app.integrations.iter_mut().filter(|i| i.kind == kind) {
            widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut integ.enabled, "");
                    ui.label(egui::RichText::new(integ.name).strong().color(if integ.enabled { theme::ACCENT } else { theme::TEXT }));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(integ.enabled && !integ.value.is_empty(), egui::Button::new("Test")).clicked() {
                            test_ping(&ctx, integ.name, integ.value.clone(), Arc::clone(&status_sink));
                        }
                    });
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(integ.field).color(theme::TEXT_MUTED).size(12.0));
                    ui.add(
                        egui::TextEdit::singleline(&mut integ.value)
                            .desired_width(420.0)
                            .password(integ.field.contains("token") || integ.field.contains("key") || integ.field.contains("DSN")),
                    );
                });
            });
            ui.add_space(4.0);
        }
        ui.add_space(6.0);
    }

    if let Some(status) = app.integration_status.lock().expect("Integration status mutex poisoned").clone() {
        ui.add_space(8.0);
        widgets::card(ui, |ui| {
            ui.label(egui::RichText::new(status).color(theme::TEXT_MUTED).monospace().size(12.0));
        });
    }

    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(
            "Note: values are held in memory for this session. Test posts a small JSON payload to URL-based webhooks.",
        )
        .color(theme::TEXT_MUTED)
        .size(11.5),
    );
}

fn test_ping(ctx: &egui::Context, name: &'static str, target: String, sink: Arc<Mutex<Option<String>>>) {
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let result = if target.starts_with("http://") || target.starts_with("https://") {
            let body = serde_json::json!({ "text": format!("neo-gui test ping for {name}"), "content": format!("neo-gui test ping for {name}") });
            match reqwest::blocking::Client::new()
                .post(&target)
                .json(&body)
                .timeout(std::time::Duration::from_secs(8))
                .send()
            {
                Ok(r) => format!("{name}: HTTP {}", r.status().as_u16()),
                Err(e) => format!("{name}: error — {e}"),
            }
        } else {
            format!("{name}: configured (no URL to test)")
        };
        if let Ok(mut s) = sink.lock() {
            *s = Some(result);
        }
        ctx.request_repaint();
    });
}
