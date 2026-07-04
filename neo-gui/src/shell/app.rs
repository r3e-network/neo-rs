//! The application shell: state, background poller, layout, and navigation.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::{Context, RichText};

use crate::node::{LocalNode, NodeState, SharedState};
use crate::rpc::RpcClient;
use crate::screens::Screen;
use crate::theme;

/// Configuration the poller thread reads each tick.
pub struct PollerCfg {
    pub url: String,
    pub interval: Duration,
    pub enabled: bool,
}

impl Default for PollerCfg {
    fn default() -> Self {
        Self {
            url: "http://127.0.0.1:10332".into(),
            interval: Duration::from_secs(2),
            enabled: true,
        }
    }
}

/// Top-level application state.
pub struct NeoGuiApp {
    pub screen: Screen,
    pub state: SharedState,
    pub cfg: Arc<Mutex<PollerCfg>>,
    pub url_edit: String,
    pub local: LocalNode,

    // RPC explorer scratch state.
    pub rpc_method: String,
    pub rpc_params: String,
    pub rpc_out: Arc<Mutex<Option<String>>>,
    pub rpc_busy: Arc<AtomicBool>,

    // Wallet screen scratch state.
    pub wallet_path: String,
    pub wallet_password: String,
    pub wallet_out: Arc<Mutex<Option<String>>>,

    // Signer screen: selected key-management backend.
    pub signer_backend: usize,

    // Configuration editor.
    pub config_text: String,
    pub config_status: Option<String>,

    // Third-party integrations.
    pub integrations: Vec<Integration>,
    pub integration_status: Arc<Mutex<Option<String>>>,
}

/// A configurable third-party monitoring / alerting / logging integration.
pub struct Integration {
    pub name: &'static str,
    pub kind: &'static str,
    pub field: &'static str,
    pub value: String,
    pub enabled: bool,
}

impl Integration {
    fn new(name: &'static str, kind: &'static str, field: &'static str) -> Self {
        Self { name, kind, field, value: String::new(), enabled: false }
    }
}

fn default_integrations() -> Vec<Integration> {
    vec![
        Integration::new("Prometheus / Grafana", "Metrics", "remote_write URL"),
        Integration::new("Datadog", "Metrics", "API key"),
        Integration::new("Better Stack Logs", "Logging", "source token"),
        Integration::new("Better Stack Uptime", "Uptime", "heartbeat URL"),
        Integration::new("UptimeRobot", "Uptime", "monitor URL / key"),
        Integration::new("Slack", "Alerting", "webhook URL"),
        Integration::new("Discord", "Alerting", "webhook URL"),
        Integration::new("Telegram", "Alerting", "bot token / chat id"),
        Integration::new("Sentry", "Errors", "DSN"),
    ]
}

impl NeoGuiApp {
    /// Build the app, install the theme, and start the background poller.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);

        let cfg = Arc::new(Mutex::new(PollerCfg::default()));
        let state: SharedState = Arc::new(Mutex::new(NodeState::default()));
        let url_edit = cfg.lock().expect("PollerCfg mutex poisoned").url.clone();

        spawn_poller(cc.egui_ctx.clone(), Arc::clone(&cfg), Arc::clone(&state));

        Self {
            screen: Screen::Dashboard,
            state,
            cfg,
            url_edit,
            local: LocalNode::default(),
            rpc_method: "getversion".into(),
            rpc_params: "[]".into(),
            rpc_out: Arc::new(Mutex::new(None)),
            rpc_busy: Arc::new(AtomicBool::new(false)),
            wallet_path: String::new(),
            wallet_password: String::new(),
            wallet_out: Arc::new(Mutex::new(None)),
            signer_backend: 0,
            config_text: String::new(),
            config_status: None,
            integrations: default_integrations(),
            integration_status: Arc::new(Mutex::new(None)),
        }
    }

    /// Run an RPC call on a worker thread, writing the result to a given sink.
    pub fn run_rpc_to(&self, ctx: &Context, method: &str, params: serde_json::Value, sink: Arc<Mutex<Option<String>>>) {
        let url = self.rpc_url();
        let method = method.to_string();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let text = match RpcClient::new(url).call(&method, params) {
                Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|e| e.to_string()),
                Err(e) => format!("error: {e}"),
            };
            if let Ok(mut o) = sink.lock() {
                *o = Some(text);
            }
            ctx.request_repaint();
        });
    }

    /// The configured RPC endpoint.
    pub fn rpc_url(&self) -> String {
        self.cfg.lock().map(|c| c.url.clone()).unwrap_or_default()
    }

    /// Run a one-off RPC call on a worker thread, writing pretty JSON to `rpc_out`.
    pub fn run_rpc(&self, ctx: &Context) {
        if self.rpc_busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let url = self.rpc_url();
        let method = self.rpc_method.trim().to_string();
        let params_raw = self.rpc_params.clone();
        let out = Arc::clone(&self.rpc_out);
        let busy = Arc::clone(&self.rpc_busy);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let params = serde_json::from_str(&params_raw)
                .unwrap_or(serde_json::Value::Array(vec![]));
            let text = match RpcClient::new(url).call(&method, params) {
                Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|e| e.to_string()),
                Err(e) => format!("error: {e}"),
            };
            if let Ok(mut o) = out.lock() {
                *o = Some(text);
            }
            busy.store(false, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }
}

impl eframe::App for NeoGuiApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.top_bar(ctx);
        self.nav(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                crate::screens::render(self, ui);
            });
        });
        // Keep the UI live so poller updates show without input.
        ctx.request_repaint_after(Duration::from_millis(750));
    }
}

impl NeoGuiApp {
    fn top_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("top")
            .exact_height(54.0)
            .frame(egui::Frame::none().fill(theme::BG_PANEL).inner_margin(egui::Margin::symmetric(16.0, 8.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("◆ Neo Node Manager").size(18.0).strong().color(theme::ACCENT));
                    ui.add_space(16.0);

                    let (online, height, network) = {
let s = self.state.lock().expect("NodeState mutex poisoned");
                    (
                        s.online,
                        s.status.as_ref().map(|st| st.block_count).unwrap_or(0),
                        s.status.as_ref().map(|st| st.version.protocol.network_name()).unwrap_or("—"),
                    )
                    };

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        crate::widgets::status_pill(ui, online);
                        ui.add_space(12.0);
                        ui.label(RichText::new(network).color(theme::TEXT_MUTED));
                        ui.add_space(12.0);
                        ui.label(RichText::new(format!("⛓ {height}")).monospace().color(theme::TEXT));
                        ui.add_space(12.0);
                        ui.label(RichText::new(self.rpc_url()).monospace().size(12.0).color(theme::TEXT_MUTED));
                    });
                });
            });
    }

    fn nav(&mut self, ctx: &Context) {
        egui::SidePanel::left("nav")
            .exact_width(196.0)
            .frame(egui::Frame::none().fill(theme::BG_PANEL).inner_margin(egui::Margin::same(10.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.add_space(4.0);
                    for (group, items) in crate::screens::NAV {
                        ui.add_space(8.0);
                        ui.label(RichText::new(*group).size(10.5).strong().color(theme::TEXT_MUTED));
                        ui.add_space(2.0);
                        for item in *items {
                            let selected = self.screen == *item;
                            let label = RichText::new(format!("  {}  {}", item.icon(), item.title()))
                                .size(14.0)
                                .color(if selected { theme::ACCENT } else { theme::TEXT });
                            let resp = ui.add_sized(
                                [ui.available_width(), 32.0],
                                egui::SelectableLabel::new(selected, label),
                            );
                            if resp.clicked() {
                                self.screen = *item;
                            }
                            ui.add_space(1.0);
                        }
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new("neo-gui v0.1").size(11.0).color(theme::TEXT_MUTED));
                });
            });
    }
}

fn spawn_poller(ctx: Context, cfg: Arc<Mutex<PollerCfg>>, state: SharedState) {
    std::thread::spawn(move || {
        let mut sys = sysinfo::System::new();
        let mut last_height: u64 = 0;
        loop {
            let (url, interval, enabled) = {
                let c = cfg.lock().expect("PollerCfg mutex poisoned");
                (c.url.clone(), c.interval, c.enabled)
            };

            // Host metrics (refreshed every tick; CPU needs the previous sample).
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            let disks = sysinfo::Disks::new_with_refreshed_list();
            let (disk_total, disk_avail) = disks
                .iter()
                .find(|d| d.mount_point().to_str() == Some("/"))
                .or_else(|| disks.iter().next())
                .map(|d| (d.total_space(), d.available_space()))
                .unwrap_or((0, 0));
            let host = crate::node::HostMetrics {
                cpu_percent: sys.global_cpu_usage(),
                mem_used: sys.used_memory(),
                mem_total: sys.total_memory(),
                disk_total,
                disk_used: disk_total.saturating_sub(disk_avail),
                process_running: false,
                uptime_secs: sysinfo::System::uptime(),
            };

            if enabled && !url.is_empty() {
                let client = RpcClient::new(&url);
                match client.status() {
                    Ok(status) => {
                        let peers = client.peers().ok();
                        let bps = status.block_count.saturating_sub(last_height) as f32;
                        last_height = status.block_count;
let mut s = state.lock().expect("NodeState mutex poisoned");
                    s.online = true;
                    s.last_error = None;
                    s.push_height(status.block_count);
                    s.push_metrics(host.cpu_percent, host.mem_percent(), bps);
                    s.status = Some(status);
                    s.peers = peers;
                    s.host = host;
                    }
                    Err(e) => {
let mut s = state.lock().expect("NodeState mutex poisoned");
                    s.online = false;
                    s.last_error = Some(e.to_string());
                    s.host = host;
                    }
                }
            } else {
let mut s = state.lock().expect("NodeState mutex poisoned");
            s.host = host;
            }
            ctx.request_repaint();
            std::thread::sleep(interval);
        }
    });
}
