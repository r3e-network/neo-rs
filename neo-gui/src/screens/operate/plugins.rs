//! Plugins: the node's plugin/feature model and the live `listplugins` view.
//!
//! In neo-rs, "plugins" are compile-time Cargo features of `neo-node` (RPC
//! server, oracle service, etc.) plus the always-present native contracts —
//! not dynamically loaded DLLs like the C# node. This screen shows the model
//! and queries the running node for what it reports.

use egui::Ui;
use serde_json::json;

use crate::app::NeoGuiApp;
use crate::runtime::sync::lock;
use crate::theme;
use crate::widgets;

const PLUGINS: &[(&str, &str, &str)] = &[
    (
        "RpcServer",
        "API",
        "JSON-RPC server (neo-rpc/server) — always on in the default node build.",
    ),
    (
        "OracleService",
        "Core",
        "HTTPS + NeoFS oracle request fulfilment (neo-oracle-service).",
    ),
    (
        "StateService",
        "Core",
        "MPT state root, proofs (getproof/getstate) (neo-state-service).",
    ),
    (
        "ApplicationLogs",
        "Core",
        "Execution logs / notifications (served via getapplicationlog).",
    ),
    (
        "TokensTracker",
        "API",
        "NEP-11/NEP-17 balance + transfer indexing (merged into neo-rpc).",
    ),
    (
        "DBFTPlugin",
        "Consensus",
        "dBFT 2.0 consensus driver (neo-consensus).",
    ),
];

pub fn ui(app: &mut NeoGuiApp, ui: &mut Ui) {
    ui.heading("Plugins");
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(
            "Node plugins are compile-time features in neo-rs. Query the node for what it exposes.",
        )
        .color(theme::TEXT_MUTED),
    );
    ui.add_space(12.0);

    widgets::section(ui, "Available plugins");
    widgets::card(ui, |ui| {
        for (name, cat, desc) in PLUGINS {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{name}"))
                        .strong()
                        .color(theme::ACCENT),
                );
                ui.label(
                    egui::RichText::new(format!("[{cat}]"))
                        .color(theme::TEXT_MUTED)
                        .size(11.5),
                );
            });
            ui.label(
                egui::RichText::new(*desc)
                    .color(theme::TEXT_MUTED)
                    .size(12.5),
            );
            ui.add_space(6.0);
        }
    });

    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if ui
            .button(egui::RichText::new("Query node (listplugins)").strong())
            .clicked()
        {
            app.run_rpc_to(ui.ctx(), "listplugins", json!([]), app.rpc_out.clone());
        }
    });
    ui.add_space(8.0);
    widgets::section(ui, "Reported by node");
    let out = lock(&app.rpc_out, "RPC output").clone();
    widgets::card(ui, |ui| match out {
        Some(mut text) => {
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .code_editor()
                    .desired_rows(12)
                    .desired_width(f32::INFINITY)
                    .interactive(false),
            );
        }
        None => {
            ui.label(
                egui::RichText::new("Query the node to see its plugin list.")
                    .color(theme::TEXT_MUTED),
            );
        }
    });
}
