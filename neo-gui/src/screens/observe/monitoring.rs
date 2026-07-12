//! Monitoring: host resources and node sync metrics with live charts.

use egui::Ui;

use crate::app::NeoGuiApp;
use crate::node::human_bytes;
use crate::runtime::sync::lock;
use crate::theme;
use crate::widgets;

pub fn ui(app: &mut NeoGuiApp, ui: &mut Ui) {
    let (host, cpu_hist, mem_hist, bps_hist, online) = {
        let s = lock(&app.state, "NodeState");
        (
            s.host,
            s.cpu_history.iter().copied().collect::<Vec<_>>(),
            s.mem_history.iter().copied().collect::<Vec<_>>(),
            s.bps_history.iter().copied().collect::<Vec<_>>(),
            s.online,
        )
    };

    ui.heading("Monitoring");
    ui.add_space(10.0);

    // Host resource gauges.
    widgets::section(ui, "Host resources");
    ui.horizontal_wrapped(|ui| {
        gauge(
            ui,
            "CPU",
            host.cpu_percent,
            format!("{:.0}%", host.cpu_percent),
        );
        gauge(
            ui,
            "Memory",
            host.mem_percent(),
            format!(
                "{} / {}",
                human_bytes(host.mem_used),
                human_bytes(host.mem_total)
            ),
        );
        gauge(
            ui,
            "Disk",
            host.disk_percent(),
            format!(
                "{} / {}",
                human_bytes(host.disk_used),
                human_bytes(host.disk_total)
            ),
        );
    });

    ui.add_space(14.0);
    widgets::section(ui, "Trends");
    ui.horizontal_wrapped(|ui| {
        chart(ui, "CPU %", &cpu_hist, theme::ACCENT, Some(100.0));
        chart(ui, "Memory %", &mem_hist, theme::WARN, Some(100.0));
        chart(ui, "Blocks / poll", &bps_hist, theme::OK, None);
    });

    ui.add_space(14.0);
    widgets::section(ui, "Alerts");
    widgets::card(ui, |ui| {
        alert_row(
            ui,
            "Node reachable",
            online,
            if online { "ok" } else { "node not responding" },
        );
        alert_row(
            ui,
            "Disk < 90%",
            host.disk_percent() < 90.0,
            &format!("{:.0}% used", host.disk_percent()),
        );
        alert_row(
            ui,
            "Memory < 90%",
            host.mem_percent() < 90.0,
            &format!("{:.0}% used", host.mem_percent()),
        );
        let syncing = bps_hist.iter().rev().take(5).any(|b| *b > 0.0);
        alert_row(
            ui,
            "Sync progressing",
            syncing || !online,
            if syncing { "advancing" } else { "stalled?" },
        );
    });
}

fn gauge(ui: &mut Ui, label: &str, percent: f32, sub: String) {
    egui::Frame::none()
        .fill(theme::BG_CARD)
        .rounding(egui::Rounding::same(10.0))
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, |ui| {
            ui.set_min_width(220.0);
            ui.label(
                egui::RichText::new(label)
                    .color(theme::TEXT_MUTED)
                    .size(12.0),
            );
            ui.add_space(6.0);
            let color = if percent > 90.0 {
                theme::ERR
            } else if percent > 75.0 {
                theme::WARN
            } else {
                theme::OK
            };
            ui.add(
                egui::ProgressBar::new((percent / 100.0).clamp(0.0, 1.0))
                    .fill(color)
                    .desired_height(12.0)
                    .text(egui::RichText::new(format!("{percent:.0}%")).strong()),
            );
            ui.add_space(4.0);
            ui.label(egui::RichText::new(sub).color(theme::TEXT_MUTED).size(11.5));
        });
}

fn chart(ui: &mut Ui, label: &str, data: &[f32], color: egui::Color32, max: Option<f64>) {
    egui::Frame::none()
        .fill(theme::BG_CARD)
        .rounding(egui::Rounding::same(10.0))
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.set_min_width(300.0);
            ui.label(
                egui::RichText::new(label)
                    .color(theme::TEXT_MUTED)
                    .size(12.0),
            );
            ui.add_space(4.0);
            let points: egui_plot::PlotPoints = data
                .iter()
                .enumerate()
                .map(|(i, v)| [i as f64, *v as f64])
                .collect();
            let mut plot = egui_plot::Plot::new(label)
                .height(90.0)
                .show_axes([false, true])
                .show_grid(false)
                .allow_drag(false)
                .allow_zoom(false)
                .allow_scroll(false);
            if let Some(m) = max {
                plot = plot.include_y(0.0).include_y(m);
            }
            plot.show(ui, |plot_ui| {
                plot_ui.line(
                    egui_plot::Line::new(points)
                        .color(color)
                        .width(2.0)
                        .fill(0.0),
                );
            });
        });
}

fn alert_row(ui: &mut Ui, label: &str, ok: bool, detail: &str) {
    ui.horizontal(|ui| {
        let (glyph, color) = if ok {
            ("●", theme::OK)
        } else {
            ("●", theme::ERR)
        };
        ui.label(egui::RichText::new(glyph).color(color));
        ui.label(egui::RichText::new(label).color(theme::TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(detail)
                    .color(theme::TEXT_MUTED)
                    .size(12.0),
            );
        });
    });
}
