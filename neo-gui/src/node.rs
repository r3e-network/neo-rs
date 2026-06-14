//! Live node state (updated by the background poller) and local process control.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::rpc::{NodeStatus, Peers};

/// Shared, poller-updated view of the connected node.
pub type SharedState = Arc<Mutex<NodeState>>;

/// Latest known node state. Read by the UI every frame.
#[derive(Default)]
pub struct NodeState {
    /// Whether the last poll reached the node.
    pub online: bool,
    /// Headline status from the last successful poll.
    pub status: Option<NodeStatus>,
    /// Peer lists from the last successful poll.
    pub peers: Option<Peers>,
    /// Last error string, if the most recent poll failed.
    pub last_error: Option<String>,
    /// Recent block heights, for the sync sparkline (height samples).
    pub height_history: VecDeque<u64>,
    /// Host machine metrics (the machine running the manager / local node).
    pub host: HostMetrics,
    /// CPU% history for the monitoring chart.
    pub cpu_history: VecDeque<f32>,
    /// Memory% history for the monitoring chart.
    pub mem_history: VecDeque<f32>,
    /// Blocks-per-poll history (sync rate) for the monitoring chart.
    pub bps_history: VecDeque<f32>,
}

/// Host machine resource snapshot.
#[derive(Default, Clone, Copy)]
pub struct HostMetrics {
    pub cpu_percent: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    pub process_running: bool,
    pub uptime_secs: u64,
}

impl HostMetrics {
    pub fn mem_percent(&self) -> f32 {
        if self.mem_total == 0 { 0.0 } else { self.mem_used as f32 / self.mem_total as f32 * 100.0 }
    }
    pub fn disk_percent(&self) -> f32 {
        if self.disk_total == 0 { 0.0 } else { self.disk_used as f32 / self.disk_total as f32 * 100.0 }
    }
}

/// Format a byte count as a human string (GiB/MiB).
pub fn human_bytes(b: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    if b >= GIB {
        format!("{:.1} GiB", b as f64 / GIB as f64)
    } else {
        format!("{:.0} MiB", b as f64 / MIB as f64)
    }
}

impl NodeState {
    /// Record a height sample, keeping a bounded window.
    pub fn push_height(&mut self, h: u64) {
        if self.height_history.back().copied() != Some(h) {
            self.height_history.push_back(h);
            while self.height_history.len() > 120 {
                self.height_history.pop_front();
            }
        }
    }

    /// Record a monitoring sample (called once per poll).
    pub fn push_metrics(&mut self, cpu: f32, mem: f32, bps: f32) {
        for (series, v) in [
            (&mut self.cpu_history, cpu),
            (&mut self.mem_history, mem),
            (&mut self.bps_history, bps),
        ] {
            series.push_back(v);
            while series.len() > 120 {
                series.pop_front();
            }
        }
    }
}

/// Controls a locally-spawned `neo-node` process.
#[derive(Default)]
pub struct LocalNode {
    /// Path to the `neo-node` binary.
    pub binary: String,
    /// Path to the TOML config file passed with `--config`.
    pub config: String,
    child: Option<Child>,
    logs: Arc<Mutex<VecDeque<String>>>,
}

impl LocalNode {
    /// True while the spawned process is alive.
    pub fn is_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Spawn the node with the configured binary + config, capturing output.
    pub fn start(&mut self) -> anyhow::Result<()> {
        if self.is_running() {
            return Ok(());
        }
        let mut cmd = Command::new(&self.binary);
        if !self.config.is_empty() {
            cmd.arg("--config").arg(&self.config);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn()?;

        for stream in [child.stdout.take().map(Capture::Out), child.stderr.take().map(Capture::Err)]
            .into_iter()
            .flatten()
        {
            let logs = Arc::clone(&self.logs);
            std::thread::spawn(move || stream.pump(&logs));
        }
        self.child = Some(child);
        Ok(())
    }

    /// Terminate the spawned process.
    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Snapshot the captured log tail.
    pub fn log_lines(&self) -> Vec<String> {
        self.logs.lock().map(|l| l.iter().cloned().collect()).unwrap_or_default()
    }
}

enum Capture {
    Out(std::process::ChildStdout),
    Err(std::process::ChildStderr),
}

impl Capture {
    fn pump(self, logs: &Arc<Mutex<VecDeque<String>>>) {
        let reader: Box<dyn BufRead> = match self {
            Capture::Out(s) => Box::new(BufReader::new(s)),
            Capture::Err(s) => Box::new(BufReader::new(s)),
        };
        for line in reader.lines().map_while(Result::ok) {
            if let Ok(mut l) = logs.lock() {
                l.push_back(line);
                while l.len() > 2000 {
                    l.pop_front();
                }
            }
        }
    }
}
