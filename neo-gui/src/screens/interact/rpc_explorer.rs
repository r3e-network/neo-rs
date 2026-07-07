//! Contracts / RPC explorer: invoke any JSON-RPC method and inspect the result.

use egui::Ui;

use crate::app::NeoGuiApp;
use crate::sync::lock;
use crate::theme;
use crate::widgets;

/// Common methods offered as quick-picks (method, default params).
const QUICK: &[(&str, &str)] = &[
    ("getversion", "[]"),
    ("getblockcount", "[]"),
    ("getbestblockhash", "[]"),
    ("getblock", "[0, true]"),
    ("getrawmempool", "[]"),
    ("getcommittee", "[]"),
    ("getnextblockvalidators", "[]"),
    ("getnativecontracts", "[]"),
    (
        "invokefunction",
        "[\"0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5\", \"symbol\", []]",
    ),
    ("getcontractstate", "[\"NeoToken\"]"),
];

pub fn ui(app: &mut NeoGuiApp, ui: &mut Ui) {
    ui.heading("Contracts & RPC");
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new("Invoke any JSON-RPC method on the connected node.")
            .color(theme::TEXT_MUTED),
    );
    ui.add_space(12.0);

    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Method").color(theme::TEXT_MUTED));
            ui.add(
                egui::TextEdit::singleline(&mut app.rpc_method)
                    .desired_width(220.0)
                    .hint_text("getversion"),
            );

            let busy = app.rpc_busy.load(std::sync::atomic::Ordering::SeqCst);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let btn = egui::Button::new(
                    egui::RichText::new(if busy { "Running…" } else { "▶ Run" }).strong(),
                );
                if ui.add_enabled(!busy, btn).clicked() {
                    app.run_rpc(ui.ctx());
                }
            });
        });
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Params (JSON array)").color(theme::TEXT_MUTED));
        ui.add(
            egui::TextEdit::multiline(&mut app.rpc_params)
                .desired_rows(2)
                .code_editor()
                .desired_width(f32::INFINITY),
        );
    });

    ui.add_space(10.0);
    ui.label(egui::RichText::new("Quick methods").color(theme::TEXT_MUTED));
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for (m, p) in QUICK {
            if ui.button(*m).clicked() {
                app.rpc_method = (*m).to_string();
                app.rpc_params = (*p).to_string();
            }
        }
    });

    ui.add_space(14.0);
    widgets::section(ui, "Result");
    let out = lock(&app.rpc_out, "RPC output").clone();
    widgets::card(ui, |ui| match out {
        Some(text) => {
            let mut text = text;
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .code_editor()
                    .desired_rows(16)
                    .desired_width(f32::INFINITY)
                    .interactive(false),
            );
        }
        None => {
            ui.label(
                egui::RichText::new("Run a method to see its result.").color(theme::TEXT_MUTED),
            );
        }
    });
}
