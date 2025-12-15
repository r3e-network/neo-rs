//! System resource monitoring

use serde::{Deserialize, Serialize};
use sysinfo::{CpuExt, Pid, PidExt, ProcessExt, System, SystemExt};

/// System resource information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    /// Total system memory in bytes
    pub total_memory: u64,

    /// Used memory in bytes
    pub used_memory: u64,

    /// Available memory in bytes
    pub available_memory: u64,

    /// Memory usage percentage
    pub memory_usage_percent: f64,

    /// CPU usage percentage (average across all cores)
    pub cpu_usage_percent: f64,

    /// Number of CPU cores
    pub cpu_count: usize,

    /// Process memory usage in bytes
    pub process_memory: u64,

    /// System uptime in seconds
    pub uptime_secs: u64,
}

/// System resource monitor
pub struct SystemMonitor {
    system: System,
}

impl SystemMonitor {
    /// Create a new system monitor
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_memory();
        system.refresh_cpu();
        Self { system }
    }

    /// Refresh system information
    pub fn refresh(&mut self) {
        self.system.refresh_memory();
        self.system.refresh_cpu();
    }

    /// Get current system info
    pub fn info(&mut self) -> SystemInfo {
        self.refresh();

        let total_memory = self.system.total_memory();
        let used_memory = self.system.used_memory();
        let available_memory = self.system.available_memory();

        let memory_usage_percent = if total_memory > 0 {
            (used_memory as f64 / total_memory as f64) * 100.0
        } else {
            0.0
        };

        let cpu_count = self.system.cpus().len();
        let cpu_usage_percent = if cpu_count > 0 {
            self.system.cpus().iter().map(|cpu| cpu.cpu_usage() as f64).sum::<f64>() / cpu_count as f64
        } else {
            0.0
        };

        // Get process memory (current process)
        let pid = std::process::id();
        let process_memory = self.system.process(Pid::from_u32(pid))
            .map(|p| p.memory())
            .unwrap_or(0);

        SystemInfo {
            total_memory,
            used_memory,
            available_memory,
            memory_usage_percent,
            cpu_usage_percent,
            cpu_count,
            process_memory,
            uptime_secs: self.system.uptime(),
        }
    }

    /// Get total system memory
    pub fn total_memory(&self) -> u64 {
        self.system.total_memory()
    }

    /// Get used memory
    pub fn used_memory(&self) -> u64 {
        self.system.used_memory()
    }

    /// Get system uptime
    pub fn uptime(&self) -> u64 {
        self.system.uptime()
    }
}

impl Default for SystemMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_monitor() {
        let mut monitor = SystemMonitor::new();
        let info = monitor.info();

        // Basic sanity checks
        assert!(info.total_memory > 0);
        assert!(info.cpu_count > 0);
        assert!(info.memory_usage_percent >= 0.0);
        assert!(info.memory_usage_percent <= 100.0);
    }

    #[test]
    fn test_uptime() {
        let monitor = SystemMonitor::new();
        let uptime = monitor.uptime();
        assert!(uptime > 0);
    }
}
