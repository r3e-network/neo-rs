use super::CommandResult;
use crate::console_service::ConsoleHelper;
use anyhow::{anyhow, Context};
use chrono::{DateTime, Local};
use crossterm::{
    cursor,
    event::{self, Event, KeyEventKind},
    queue,
    style::{self, Color, SetForegroundColor},
    terminal::{self, ClearType},
};
use neo_core::{
    neo_system::NeoSystem,
    network::p2p::{
        message::Message, message_command::MessageCommand, payloads::ping_payload::PingPayload,
    },
};
use std::{
    cmp,
    io::{stdout, Stdout, Write},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use sysinfo::{self, PidExt, ProcessExt, System, SystemExt};
use tokio::runtime::Handle;

/// Node lifecycle commands (`MainService.Node`).
pub struct NodeCommands {
    system: Arc<NeoSystem>,
}

impl NodeCommands {
    pub fn new(system: Arc<NeoSystem>) -> Self {
        Self { system }
    }

    /// Displays mempool state (parity with C# `show pool`).
    pub fn show_pool(&self, verbose: bool) -> CommandResult {
        let mempool = self.system.mempool();
        let pool = mempool
            .lock()
            .map_err(|_| anyhow!("failed to access mempool"))?;

        if verbose {
            ConsoleHelper::info(["Verified Transactions:"]);
            for tx in pool.verified_transactions_vec() {
                let kind = "Transaction";
                ConsoleHelper::info([
                    " ",
                    &tx.hash().to_string(),
                    " ",
                    kind,
                    " ",
                    &format!("{}", tx.network_fee()),
                    " GAS_NetFee",
                ]);
            }

            ConsoleHelper::info(["Unverified Transactions:"]);
            for tx in pool.unverified_transactions_vec() {
                let kind = "Transaction";
                ConsoleHelper::info([
                    " ",
                    &tx.hash().to_string(),
                    " ",
                    kind,
                    " ",
                    &format!("{}", tx.network_fee()),
                    " GAS_NetFee",
                ]);
            }
        }

        ConsoleHelper::info([&format!(
            "total: {}, verified: {}, unverified: {}",
            pool.count(),
            pool.verified_count(),
            pool.unverified_count()
        )]);

        Ok(())
    }

    /// Displays the interactive node dashboard (parity with C# `show state`).
    pub fn show_state(&self) -> CommandResult {
        let handle =
            Handle::try_current().map_err(|err| anyhow!("tokio runtime unavailable: {err}"))?;
        let mut terminal = TerminalGuard::enter()?;
        let mut shower = StateShower::new(Arc::clone(&self.system));
        let mut next_ping = Instant::now();
        let ping_interval = self.ping_interval();

        loop {
            let snapshot = shower.capture_snapshot(&handle);
            let now = Instant::now();

            if shower.should_refresh(&snapshot, now) {
                if !StateShower::validate_console_window()? {
                    shower.render_resize_warning(terminal.stdout_mut())?;
                    thread::sleep(Duration::from_millis(500));
                } else if let Err(err) = shower.render_snapshot(&snapshot, terminal.stdout_mut()) {
                    shower.handle_render_error(&err, terminal.stdout_mut())?;
                    thread::sleep(Duration::from_millis(1000));
                }
            }

            if now >= next_ping {
                shower.broadcast_ping();
                next_ping = now + ping_interval;
            }

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    fn ping_interval(&self) -> Duration {
        let base = self.system.time_per_block();
        let seconds = base.as_secs_f64() / 4.0;
        Duration::from_secs_f64(seconds.max(0.25))
    }
}

struct TerminalGuard {
    stdout: Stdout,
}

impl TerminalGuard {
    fn enter() -> anyhow::Result<Self> {
        terminal::enable_raw_mode().context("failed to enable raw mode")?;
        let mut stdout = stdout();
        queue!(
            stdout,
            cursor::Hide,
            terminal::Clear(ClearType::All),
            cursor::MoveTo(0, 0)
        )
        .context("failed to prepare console")?;
        stdout.flush().context("failed to flush console")?;
        Ok(Self { stdout })
    }

    fn stdout_mut(&mut self) -> &mut Stdout {
        &mut self.stdout
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = queue!(
            self.stdout,
            SetForegroundColor(Color::Reset),
            terminal::Clear(ClearType::All),
            cursor::MoveTo(0, 0),
            cursor::Show
        );
        let _ = self.stdout.flush();
        let _ = terminal::disable_raw_mode();
    }
}

struct StateShower {
    system: Arc<NeoSystem>,
    display_state: DisplayState,
    monitor: ResourceMonitor,
    lines: Vec<LineEntry>,
    max_lines: usize,
}

impl StateShower {
    fn new(system: Arc<NeoSystem>) -> Self {
        Self {
            system,
            display_state: DisplayState::new(),
            monitor: ResourceMonitor::new(),
            lines: Vec::new(),
            max_lines: 0,
        }
    }

    fn capture_snapshot(&mut self, handle: &Handle) -> NodeSnapshot {
        let block_height = self.system.current_block_index();
        let header_cache = self.system.context().header_cache();
        let header_height = header_cache
            .last()
            .map(|header| header.index())
            .unwrap_or(block_height);
        let (mempool_total, mempool_verified, mempool_unverified) = self
            .system
            .mempool()
            .lock()
            .map(|pool| (pool.count(), pool.verified_count(), pool.unverified_count()))
            .unwrap_or((0, 0, 0));

        let connected_count = handle.block_on(self.system.peer_count()).unwrap_or(0);
        let unconnected_count = handle
            .block_on(self.system.unconnected_count())
            .unwrap_or(0);
        let max_peer_height = handle
            .block_on(self.system.max_peer_block_height())
            .unwrap_or(0);

        self.monitor.refresh();

        NodeSnapshot {
            now: Local::now(),
            block_height,
            header_height,
            mempool_total,
            mempool_verified,
            mempool_unverified,
            connected_count,
            unconnected_count,
            max_peer_height,
            uptime: self.display_state.start_time.elapsed(),
            memory_mb: self.monitor.memory_mb(),
            cpu_usage: self.monitor.cpu_usage(),
        }
    }

    fn should_refresh(&self, snapshot: &NodeSnapshot, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.display_state.last_refresh);
        elapsed >= DisplayState::REFRESH_INTERVAL
            || snapshot.block_height != self.display_state.last_height
            || snapshot.header_height != self.display_state.last_header_height
            || snapshot.mempool_total != self.display_state.last_tx_pool_size
            || snapshot.connected_count != self.display_state.last_connected_count
    }

    fn render_snapshot(
        &mut self,
        snapshot: &NodeSnapshot,
        stdout: &mut Stdout,
    ) -> std::io::Result<()> {
        let (width, height) = terminal::size()?;
        let mut box_width = width.saturating_sub(2) as usize;
        box_width = box_width.clamp(10, 70);

        self.lines.clear();
        self.render_title_box(box_width);
        self.render_time_and_uptime(box_width, snapshot);
        self.render_blockchain_and_resources(box_width, snapshot);
        self.render_transaction_and_network(box_width, snapshot);
        self.render_sync_progress(box_width, snapshot);
        self.render_footer(box_width, height);

        self.max_lines = cmp::max(self.max_lines, self.lines.len());
        self.flush(stdout, box_width, height)?;
        self.display_state.update(snapshot);
        Ok(())
    }

    fn render_resize_warning(&mut self, stdout: &mut Stdout) -> CommandResult {
        queue!(
            stdout,
            cursor::MoveTo(0, 0),
            terminal::Clear(ClearType::All),
            SetForegroundColor(Color::Red),
            style::Print("Console window too small (Need at least 70x23 visible)..."),
            SetForegroundColor(Color::Reset)
        )
        .context("failed to render resize warning")?;
        stdout.flush().context("failed to flush resize warning")?;
        self.max_lines = 1;
        Ok(())
    }

    fn handle_render_error(
        &mut self,
        error: &std::io::Error,
        stdout: &mut Stdout,
    ) -> CommandResult {
        queue!(
            stdout,
            cursor::MoveTo(0, 0),
            terminal::Clear(ClearType::All),
            SetForegroundColor(Color::Red),
            style::Print(format!("Render error: {}", error)),
            SetForegroundColor(Color::Reset)
        )
        .context("failed to render error message")?;
        stdout.flush().context("failed to flush error message")?;
        Ok(())
    }

    fn broadcast_ping(&self) {
        let payload = PingPayload::create(self.system.current_block_index());
        if let Ok(message) = Message::create(MessageCommand::Ping, Some(&payload), false) {
            let _ = self.system.local_node_actor().tell(message);
        }
    }

    fn flush(&mut self, stdout: &mut Stdout, box_width: usize, height: u16) -> std::io::Result<()> {
        let max_lines = cmp::min(self.max_lines, height.saturating_sub(1) as usize);
        for (index, entry) in self.lines.iter().enumerate().take(max_lines) {
            queue!(
                stdout,
                cursor::MoveTo(0, index as u16),
                terminal::Clear(ClearType::CurrentLine)
            )?;
            let padded = Self::pad_line(&entry.text, box_width);
            queue!(
                stdout,
                SetForegroundColor(entry.color),
                style::Print(padded),
                SetForegroundColor(Color::Reset)
            )?;
        }

        for index in self.lines.len()..max_lines {
            queue!(
                stdout,
                cursor::MoveTo(0, index as u16),
                terminal::Clear(ClearType::CurrentLine)
            )?;
        }

        stdout.flush()?;
        self.lines.clear();
        Ok(())
    }

    fn render_title_box(&mut self, box_width: usize) {
        let inner_width = box_width.saturating_sub(2);
        let horizontal = "─".repeat(inner_width);
        self.lines
            .push(LineEntry::new(format!("┌{horizontal}┐"), Color::DarkGreen));
        let title = Self::center_text("           NEO NODE STATUS             ", inner_width);
        self.lines
            .push(LineEntry::new(format!("│{title}│"), Color::DarkGreen));
        self.lines
            .push(LineEntry::new(format!("├{horizontal}┤"), Color::DarkGrey));
    }

    fn render_time_and_uptime(&mut self, box_width: usize, snapshot: &NodeSnapshot) {
        let content_width = box_width.saturating_sub(2);
        let time = snapshot.now.format("%Y-%m-%d %H:%M:%S");
        let uptime = Self::format_uptime(snapshot.uptime);
        let text = format!(" Current Time: {time}   Uptime: {uptime}");
        let padded = Self::pad_column(&text, content_width);
        self.lines
            .push(LineEntry::new(format!("│{padded}│"), Color::Grey));
    }

    fn render_blockchain_and_resources(&mut self, box_width: usize, snapshot: &NodeSnapshot) {
        let total_horizontal = box_width.saturating_sub(3);
        let left = total_horizontal / 2;
        let right = total_horizontal - left;
        self.render_split_line(left, right, "┬", "├", "┤");
        self.render_section_headers(" BLOCKCHAIN STATUS", " SYSTEM RESOURCES", left, right);
        self.render_split_line(left, right, "┼", "├", "┤");

        let height_text = format!(" Block Height:   {:>10}", snapshot.block_height);
        let memory_text = format!(" Memory Usage:   {:>10} MB", snapshot.memory_mb);
        let header_text = format!(" Header Height:  {:>10}", snapshot.header_height);
        let cpu_text = format!(" CPU Usage:      {:>10.1} %", snapshot.cpu_usage);

        self.lines.push(LineEntry::new(
            self.join_columns(&height_text, &memory_text, left, right),
            Color::Cyan,
        ));
        self.lines.push(LineEntry::new(
            self.join_columns(&header_text, &cpu_text, left, right),
            Color::Cyan,
        ));
    }

    fn render_transaction_and_network(&mut self, box_width: usize, snapshot: &NodeSnapshot) {
        let total_horizontal = box_width.saturating_sub(3);
        let left = total_horizontal / 2;
        let right = total_horizontal - left;
        self.render_split_line(left, right, "┼", "├", "┤");
        self.render_section_headers(" TRANSACTION POOL", " NETWORK STATUS", left, right);
        self.render_split_line(left, right, "┼", "├", "┤");

        let total_txs = format!(" Total Txs:      {:>10}", snapshot.mempool_total);
        let connected = format!(" Connected:      {:>10}", snapshot.connected_count);
        let verified = format!(" Verified Txs:   {:>10}", snapshot.mempool_verified);
        let unconnected = format!(" Unconnected:    {:>10}", snapshot.unconnected_count);

        self.lines.push(LineEntry::new(
            self.join_columns(&total_txs, &connected, left, right),
            Self::color_for_value(snapshot.mempool_total, 100, 500),
        ));
        self.lines.push(LineEntry::new(
            self.join_columns(&verified, &unconnected, left, right),
            Color::Green,
        ));

        let unverified = format!(" Unverified Txs: {:>10}", snapshot.mempool_unverified);
        let max_height = format!(" Max Block Height: {:>8}", snapshot.max_peer_height);
        self.lines.push(LineEntry::new(
            self.join_columns(&unverified, &max_height, left, right),
            Color::Yellow,
        ));

        self.render_split_line(left, right, "┴", "└", "┘");
    }

    fn render_sync_progress(&mut self, box_width: usize, snapshot: &NodeSnapshot) {
        if snapshot.max_peer_height > 0 && snapshot.block_height < snapshot.max_peer_height {
            let line =
                Self::progress_bar(snapshot.block_height, snapshot.max_peer_height, box_width);
            self.lines.push(LineEntry::new(line, Color::Yellow));
        }
    }

    fn render_footer(&mut self, box_width: usize, height: u16) {
        let mut footer =
            "Press any key to exit | Refresh: every 1 second or on blockchain change".to_string();
        if footer.chars().count() > box_width {
            footer = format!(
                "{}...",
                footer
                    .chars()
                    .take(box_width.saturating_sub(3))
                    .collect::<String>()
            );
        }

        let lines_written = self.lines.len();
        let footer_pos = cmp::min(height.saturating_sub(2) as usize, lines_written + 1);
        while self.lines.len() <= footer_pos {
            self.lines.push(LineEntry::new(String::new(), Color::Reset));
        }
        self.lines[footer_pos] = LineEntry::new(footer, Color::DarkGreen);
    }

    fn render_split_line(
        &mut self,
        left: usize,
        right: usize,
        middle: &str,
        start: &str,
        end: &str,
    ) {
        let line = format!(
            "{start}{left_line}{middle}{right_line}{end}",
            left_line = "─".repeat(left),
            right_line = "─".repeat(right)
        );
        self.lines.push(LineEntry::new(line, Color::DarkGrey));
    }

    fn render_section_headers(
        &mut self,
        left_header: &str,
        right_header: &str,
        left: usize,
        right: usize,
    ) {
        let left_text = Self::pad_column(left_header, left);
        let right_text = Self::pad_column(right_header, right);
        self.lines.push(LineEntry::new(
            format!("│{left_text}│{right_text}│"),
            Color::White,
        ));
    }

    fn join_columns(&self, left_text: &str, right_text: &str, left: usize, right: usize) -> String {
        let left_col = Self::pad_column(left_text, left);
        let right_col = Self::pad_column(right_text, right);
        format!("│{left_col}│{right_col}│")
    }

    fn pad_column(value: &str, width: usize) -> String {
        let chars = value.chars().count();
        if chars <= width {
            let mut result = value.to_string();
            result.extend(std::iter::repeat(' ').take(width.saturating_sub(chars)));
            result
        } else if width > 3 {
            let mut truncated = value.chars().take(width - 3).collect::<String>();
            truncated.push_str("...");
            truncated
        } else {
            value.chars().take(width).collect()
        }
    }

    fn pad_line(value: &str, width: usize) -> String {
        let chars = value.chars().count();
        if chars >= width {
            value.chars().take(width).collect()
        } else {
            let mut result = value.to_string();
            result.extend(std::iter::repeat(' ').take(width - chars));
            result
        }
    }

    fn center_text(value: &str, width: usize) -> String {
        if value.chars().count() >= width {
            return Self::pad_column(value, width);
        }
        let padding = width.saturating_sub(value.chars().count());
        let left = padding / 2;
        let right = padding - left;
        format!(
            "{left_pad}{value}{right_pad}",
            left_pad = " ".repeat(left),
            right_pad = " ".repeat(right)
        )
    }

    fn format_uptime(duration: Duration) -> String {
        let days = duration.as_secs() / 86_400;
        let hours = (duration.as_secs() % 86_400) / 3600;
        let minutes = (duration.as_secs() % 3600) / 60;
        let seconds = duration.as_secs() % 60;
        format!("{days}d {hours:02}h {minutes:02}m {seconds:02}s")
    }

    fn color_for_value(value: usize, low: usize, high: usize) -> Color {
        if value < low {
            Color::Green
        } else if value < high {
            Color::Yellow
        } else {
            Color::Red
        }
    }

    fn progress_bar(current: u32, max: u32, box_width: usize) -> String {
        let ratio = current as f64 / max as f64;
        let percentage = (ratio * 100.0).min(100.0);
        let mut bar_width = box_width.saturating_sub(25);
        if bar_width < 10 {
            bar_width = 10;
        }
        let filled = (bar_width as f64 * ratio).round() as usize;
        let clamped_filled = cmp::min(filled, bar_width);
        let filled_str = "█".repeat(clamped_filled);
        let empty_str = "░".repeat(bar_width.saturating_sub(clamped_filled));
        let mut text =
            format!(" Syncing: [{filled_str}{empty_str}] {percentage:>6.2}% ({current}/{max})");

        if text.chars().count() > box_width.saturating_sub(2) {
            text = format!(
                " Syncing: [{filled_str}{empty_str}] {percentage:>6.2}%",
                filled_str = "█".repeat(cmp::min(clamped_filled, bar_width.saturating_sub(5)))
            );
        }
        Self::pad_column(&text, box_width.saturating_sub(2))
    }

    fn validate_console_window() -> anyhow::Result<bool> {
        let (width, height) = terminal::size().context("failed to read console size")?;
        Ok(width >= 70 && height >= 23)
    }
}

struct DisplayState {
    start_time: Instant,
    last_refresh: Instant,
    last_height: u32,
    last_header_height: u32,
    last_tx_pool_size: usize,
    last_connected_count: usize,
}

impl DisplayState {
    const REFRESH_INTERVAL: Duration = Duration::from_millis(1000);

    fn new() -> Self {
        Self {
            start_time: Instant::now(),
            last_refresh: Instant::now() - Self::REFRESH_INTERVAL,
            last_height: 0,
            last_header_height: 0,
            last_tx_pool_size: 0,
            last_connected_count: 0,
        }
    }

    fn update(&mut self, snapshot: &NodeSnapshot) {
        self.last_refresh = Instant::now();
        self.last_height = snapshot.block_height;
        self.last_header_height = snapshot.header_height;
        self.last_tx_pool_size = snapshot.mempool_total;
        self.last_connected_count = snapshot.connected_count;
    }
}

struct NodeSnapshot {
    now: DateTime<Local>,
    block_height: u32,
    header_height: u32,
    mempool_total: usize,
    mempool_verified: usize,
    mempool_unverified: usize,
    connected_count: usize,
    unconnected_count: usize,
    max_peer_height: u32,
    uptime: Duration,
    memory_mb: u64,
    cpu_usage: f32,
}

struct LineEntry {
    text: String,
    color: Color,
}

impl LineEntry {
    fn new(text: String, color: Color) -> Self {
        Self { text, color }
    }
}

struct ResourceMonitor {
    system: System,
    pid: sysinfo::Pid,
}

impl ResourceMonitor {
    fn new() -> Self {
        let pid = sysinfo::get_current_pid().unwrap_or_else(|_| sysinfo::Pid::from_u32(0));
        let mut system = System::new();
        system.refresh_process(pid);
        Self { system, pid }
    }

    fn refresh(&mut self) {
        if !self.system.refresh_process(self.pid) {
            self.system.refresh_processes();
        }
    }

    fn memory_mb(&self) -> u64 {
        self.system
            .process(self.pid)
            .map(|proc| proc.memory() / 1024)
            .unwrap_or(0)
    }

    fn cpu_usage(&self) -> f32 {
        self.system
            .process(self.pid)
            .map(|proc| proc.cpu_usage())
            .unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_column_truncates_with_ellipsis() {
        let text = StateShower::pad_column("abcdefgh", 5);
        assert_eq!(text, "ab...");
    }

    #[test]
    fn progress_bar_respects_width() {
        let line = StateShower::progress_bar(50, 100, 40);
        assert!(line.contains("50"));
        assert!(line.contains("%"));
        assert!(line.chars().count() <= 38);
    }
}
