//! Screen routing and the individual views.

mod dashboard;
mod network;
mod node_control;
mod rpc_explorer;
mod settings;
mod signer;
mod wallet;

use egui::Ui;

use crate::app::NeoGuiApp;

/// The navigable sections of the manager.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Network,
    Wallet,
    Contracts,
    Node,
    Signer,
    Settings,
}

impl Screen {
    /// All screens, in sidebar order.
    pub const ALL: &'static [Screen] = &[
        Screen::Dashboard,
        Screen::Network,
        Screen::Wallet,
        Screen::Contracts,
        Screen::Node,
        Screen::Signer,
        Screen::Settings,
    ];

    /// Sidebar label.
    pub fn title(&self) -> &'static str {
        match self {
            Screen::Dashboard => "Dashboard",
            Screen::Network => "Network",
            Screen::Wallet => "Wallet",
            Screen::Contracts => "Contracts",
            Screen::Node => "Node",
            Screen::Signer => "Signer",
            Screen::Settings => "Settings",
        }
    }

    /// Sidebar glyph.
    pub fn icon(&self) -> &'static str {
        match self {
            Screen::Dashboard => "▦",
            Screen::Network => "🌐",
            Screen::Wallet => "👛",
            Screen::Contracts => "⚙",
            Screen::Node => "🖥",
            Screen::Signer => "🔑",
            Screen::Settings => "⚙",
        }
    }
}

/// Render the active screen.
pub fn render(app: &mut NeoGuiApp, ui: &mut Ui) {
    ui.add_space(6.0);
    match app.screen {
        Screen::Dashboard => dashboard::ui(app, ui),
        Screen::Network => network::ui(app, ui),
        Screen::Wallet => wallet::ui(app, ui),
        Screen::Contracts => rpc_explorer::ui(app, ui),
        Screen::Node => node_control::ui(app, ui),
        Screen::Signer => signer::ui(app, ui),
        Screen::Settings => settings::ui(app, ui),
    }
}
