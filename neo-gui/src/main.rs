//! neo-gui — a native desktop manager for the neo-rs Neo N3 node.
//!
//! The GUI is a pure client: it talks to a running node over JSON-RPC and (for
//! local nodes) controls the `neo-node` process. It links no node-internal
//! crates, so it builds and ships independently of the workspace.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod node;
mod rpc;
mod theme;
mod widgets;

mod screens;

use app::NeoGuiApp;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "neo_gui=info".into()),
        )
        .init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 560.0])
            .with_title("Neo Node Manager"),
        ..Default::default()
    };

    eframe::run_native(
        "neo-gui",
        native_options,
        Box::new(|cc| Ok(Box::new(NeoGuiApp::new(cc)))),
    )
}
