//! Network: connected/known peers and connection counts.

use egui::Ui;
use egui_extras::{Column, TableBuilder};

use crate::app::NeoGuiApp;
use crate::rpc::Peer;
use crate::runtime::sync::lock;
use crate::theme;
use crate::widgets;

pub fn ui(app: &mut NeoGuiApp, ui: &mut Ui) {
    let (peers, conns) = {
        let s = lock(&app.state, "NodeState");
        (
            s.peers.clone(),
            s.status.as_ref().map(|st| st.connections).unwrap_or(0),
        )
    };

    ui.heading("Network");
    ui.add_space(10.0);

    ui.horizontal_wrapped(|ui| {
        let p = peers.clone().unwrap_or_default();
        widgets::stat_card(ui, "Connected", conns.to_string(), theme::OK);
        widgets::stat_card(
            ui,
            "Known",
            p.unconnected.len().to_string(),
            theme::ACCENT_DIM,
        );
        widgets::stat_card(ui, "Bad", p.bad.len().to_string(), theme::ERR);
    });
    ui.add_space(14.0);

    let Some(p) = peers else {
        widgets::card(ui, |ui| {
            ui.label(
                egui::RichText::new("No peer data — is the node connected?")
                    .color(theme::TEXT_MUTED),
            );
        });
        return;
    };

    widgets::section(ui, "Connected peers");
    peer_table(ui, "connected", &p.connected);

    if !p.unconnected.is_empty() {
        ui.add_space(14.0);
        widgets::section(ui, "Known (unconnected) peers");
        peer_table(ui, "unconnected", &p.unconnected);
    }
}

fn peer_table(ui: &mut Ui, id: &str, peers: &[Peer]) {
    if peers.is_empty() {
        ui.label(egui::RichText::new("none").color(theme::TEXT_MUTED));
        return;
    }
    widgets::card(ui, |ui| {
        TableBuilder::new(ui)
            .id_salt(id)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::remainder())
            .column(Column::auto().at_least(80.0))
            .header(22.0, |mut header| {
                header.col(|ui| {
                    ui.label(
                        egui::RichText::new("Address")
                            .strong()
                            .color(theme::TEXT_MUTED),
                    );
                });
                header.col(|ui| {
                    ui.label(
                        egui::RichText::new("Port")
                            .strong()
                            .color(theme::TEXT_MUTED),
                    );
                });
            })
            .body(|mut body| {
                for peer in peers {
                    body.row(22.0, |mut row| {
                        row.col(|ui| {
                            ui.label(egui::RichText::new(&peer.address).monospace());
                        });
                        row.col(|ui| {
                            ui.label(egui::RichText::new(peer.port.to_string()).monospace());
                        });
                    });
                }
            });
    });
}
