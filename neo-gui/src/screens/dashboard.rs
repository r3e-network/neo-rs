//! Dashboard: headline status cards, sync chart, and protocol info.

use egui::Ui;

use crate::app::NeoGuiApp;
use crate::theme;
use crate::widgets;

pub fn ui(app: &mut NeoGuiApp, ui: &mut Ui) {
    // Snapshot state, then release the lock before drawing.
    let (online, status, mempool, conns, heights, last_err) = {
        let s = app.state.lock().expect("NodeState mutex poisoned");
        (
            s.online,
            s.status.clone(),
            s.status.as_ref().map(|st| st.mempool).unwrap_or(0),
            s.status.as_ref().map(|st| st.connections).unwrap_or(0),
            s.height_history.iter().copied().collect::<Vec<_>>(),
            s.last_error.clone(),
        )
    };

    ui.heading("Dashboard");
    ui.add_space(10.0);

    if !online {
        widgets::card(ui, |ui| {
            ui.label(egui::RichText::new("Not connected").size(15.0).strong().color(theme::ERR));
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    last_err.unwrap_or_else(|| "Set the RPC endpoint in Settings and ensure the node is running.".into()),
                )
                .color(theme::TEXT_MUTED),
            );
        });
        return;
    }

    let st = status.unwrap_or_default();

    // Stat tiles, flowing.
    ui.horizontal_wrapped(|ui| {
        widgets::stat_card(ui, "Block height", st.block_count.to_string(), theme::ACCENT);
        widgets::stat_card(ui, "Headers", st.header_count.to_string(), theme::ACCENT_DIM);
        widgets::stat_card(ui, "Peers", conns.to_string(), theme::OK);
        widgets::stat_card(ui, "Mempool", mempool.to_string(), theme::WARN);
        widgets::stat_card(ui, "Network", st.version.protocol.network_name(), theme::ACCENT);
        widgets::stat_card(ui, "Validators", st.version.protocol.validatorscount.to_string(), theme::ACCENT_DIM);
    });

    ui.add_space(14.0);

    // Sync progress.
    let synced = st.block_count >= st.header_count.saturating_sub(1);
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Sync").size(15.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (txt, color) = if synced {
                    ("synced".to_string(), theme::OK)
                } else {
                    (format!("{} behind", st.header_count.saturating_sub(st.block_count)), theme::WARN)
                };
                ui.label(egui::RichText::new(txt).color(color).strong());
            });
        });
        ui.add_space(6.0);
        let frac = if st.header_count == 0 {
            0.0
        } else {
            (st.block_count as f32 / st.header_count as f32).clamp(0.0, 1.0)
        };
        ui.add(egui::ProgressBar::new(frac).fill(theme::ACCENT).desired_height(10.0));

        if heights.len() >= 2 {
            ui.add_space(10.0);
            let points: egui_plot::PlotPoints = heights
                .iter()
                .enumerate()
                .map(|(i, h)| [i as f64, *h as f64])
                .collect();
            egui_plot::Plot::new("height_plot")
                .height(110.0)
                .show_axes([false, true])
                .show_grid(false)
                .allow_drag(false)
                .allow_zoom(false)
                .allow_scroll(false)
                .show(ui, |plot_ui| {
                    plot_ui.line(
                        egui_plot::Line::new(points)
                            .color(theme::ACCENT)
                            .width(2.0)
                            .name("height"),
                    );
                });
        }
    });

    ui.add_space(14.0);

    // Protocol / version detail.
    widgets::section(ui, "Protocol");
    widgets::card(ui, |ui| {
        widgets::kv(ui, "User agent", st.version.useragent.clone());
        widgets::kv(ui, "Network magic", st.version.protocol.network.to_string());
        widgets::kv(ui, "Address version", st.version.protocol.addressversion.to_string());
        widgets::kv(ui, "ms / block", st.version.protocol.msperblock.to_string());
        widgets::kv(ui, "Best block hash", st.best_hash.clone());
    });
}
