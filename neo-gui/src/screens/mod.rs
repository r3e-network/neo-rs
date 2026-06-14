//! Screen routing and the individual views.

mod configuration;
mod dashboard;
mod integrations;
mod monitoring;
mod network;
mod node_control;
mod plugins;
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
    // Operate
    Node,
    Configuration,
    Plugins,
    // Observe
    Monitoring,
    Network,
    Integrations,
    // Interact
    Contracts,
    Wallet,
    // Secure
    Keys,
    // System
    Settings,
}

/// Grouped navigation, in sidebar order.
pub const NAV: &[(&str, &[Screen])] = &[
    ("OVERVIEW", &[Screen::Dashboard]),
    ("OPERATE", &[Screen::Node, Screen::Configuration, Screen::Plugins]),
    ("OBSERVE", &[Screen::Monitoring, Screen::Network, Screen::Integrations]),
    ("INTERACT", &[Screen::Contracts, Screen::Wallet]),
    ("SECURE", &[Screen::Keys]),
    ("SYSTEM", &[Screen::Settings]),
];

impl Screen {
    /// Sidebar label.
    pub fn title(&self) -> &'static str {
        match self {
            Screen::Dashboard => "Dashboard",
            Screen::Node => "Node",
            Screen::Configuration => "Configuration",
            Screen::Plugins => "Plugins",
            Screen::Monitoring => "Monitoring",
            Screen::Network => "Network",
            Screen::Integrations => "Integrations",
            Screen::Contracts => "Contracts",
            Screen::Wallet => "Wallet",
            Screen::Keys => "Keys & Protection",
            Screen::Settings => "Settings",
        }
    }

    /// Sidebar glyph.
    pub fn icon(&self) -> &'static str {
        match self {
            Screen::Dashboard => "▦",
            Screen::Node => "🖥",
            Screen::Configuration => "🛠",
            Screen::Plugins => "🧩",
            Screen::Monitoring => "📈",
            Screen::Network => "🌐",
            Screen::Integrations => "🔌",
            Screen::Contracts => "⚙",
            Screen::Wallet => "👛",
            Screen::Keys => "🔑",
            Screen::Settings => "⚙",
        }
    }
}

/// Render the active screen.
pub fn render(app: &mut NeoGuiApp, ui: &mut Ui) {
    ui.add_space(6.0);
    match app.screen {
        Screen::Dashboard => dashboard::ui(app, ui),
        Screen::Node => node_control::ui(app, ui),
        Screen::Configuration => configuration::ui(app, ui),
        Screen::Plugins => plugins::ui(app, ui),
        Screen::Monitoring => monitoring::ui(app, ui),
        Screen::Network => network::ui(app, ui),
        Screen::Integrations => integrations::ui(app, ui),
        Screen::Contracts => rpc_explorer::ui(app, ui),
        Screen::Wallet => wallet::ui(app, ui),
        Screen::Keys => signer::ui(app, ui),
        Screen::Settings => settings::ui(app, ui),
    }
}
