//! Wallet: open a NEP-6 wallet on the node and inspect its accounts.
//!
//! These operations use the node's wallet JSON-RPC methods, which are disabled
//! by default for safety (`openwallet` is in `disabled_methods`). Enable them
//! on a trusted, loopback-only node to use this screen.

use egui::Ui;
use serde_json::json;

use crate::app::NeoGuiApp;
use crate::sync::lock;
use crate::theme;
use crate::widgets;

pub fn ui(app: &mut NeoGuiApp, ui: &mut Ui) {
    ui.heading("Wallet");
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(
            "Open a NEP-6 wallet on the node and view its accounts. Requires wallet RPC methods enabled.",
        )
        .color(theme::TEXT_MUTED),
    );
    ui.add_space(12.0);

    widgets::card(ui, |ui| {
        egui::Grid::new("wallet_open")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Wallet path").color(theme::TEXT_MUTED));
                ui.add(
                    egui::TextEdit::singleline(&mut app.wallet_path)
                        .hint_text("/path/to/wallet.json")
                        .desired_width(380.0),
                );
                ui.end_row();
                ui.label(egui::RichText::new("Password").color(theme::TEXT_MUTED));
                ui.add(
                    egui::TextEdit::singleline(&mut app.wallet_password)
                        .password(true)
                        .desired_width(380.0),
                );
                ui.end_row();
            });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui
                .button(egui::RichText::new("Open wallet").strong())
                .clicked()
            {
                let params = json!([app.wallet_path.clone(), app.wallet_password.clone()]);
                app.run_rpc_to(ui.ctx(), "openwallet", params, app.wallet_out.clone());
            }
            if ui.button("List addresses").clicked() {
                app.run_rpc_to(ui.ctx(), "listaddress", json!([]), app.wallet_out.clone());
            }
            if ui.button("Balances").clicked() {
                app.run_rpc_to(
                    ui.ctx(),
                    "getwalletbalance",
                    json!([]),
                    app.wallet_out.clone(),
                );
            }
            if ui.button("Close wallet").clicked() {
                app.run_rpc_to(ui.ctx(), "closewallet", json!([]), app.wallet_out.clone());
            }
        });
    });

    ui.add_space(14.0);
    widgets::section(ui, "Result");
    let out = lock(&app.wallet_out, "Wallet output").clone();
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
            ui.label(egui::RichText::new("Open a wallet to begin.").color(theme::TEXT_MUTED));
        }
    });
}
