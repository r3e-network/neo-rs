//! Monitoring Dashboard for Neo-RS
//!
//! This module provides a web-based dashboard for monitoring blockchain metrics
//! in real-time using the system monitoring infrastructure.

use neo_core::system_monitoring::{SYSTEM_MONITOR, SystemMetricsSnapshot};
use std::sync::Arc;
use std::time::Duration;
use std::thread;
use std::collections::VecDeque;
use serde::{Serialize, Deserialize};

/// Dashboard configuration
#[derive(Debug, Clone)]
pub struct DashboardConfig {
    /// Port to run the dashboard on
    pub port: u16,
    /// Update interval in seconds
    pub update_interval: u64,
    /// Number of historical data points to keep
    pub history_size: usize,
    /// Enable debug logging
    pub debug: bool,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            update_interval: 1,
            history_size: 60,
            debug: false,
        }
    }
}

/// Dashboard server
pub struct MonitoringDashboard {
    config: DashboardConfig,
    metrics_history: Arc<std::sync::RwLock<MetricsHistory>>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl MonitoringDashboard {
    /// Create a new monitoring dashboard
    pub fn new(config: DashboardConfig) -> Self {
        Self {
            config: config.clone(),
            metrics_history: Arc::new(std::sync::RwLock::new(
                MetricsHistory::new(config.history_size)
            )),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
    
    /// Start the dashboard server
    pub fn start(&self) -> Result<(), DashboardError> {
        if self.running.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(DashboardError::AlreadyRunning);
        }
        
        self.running.store(true, std::sync::atomic::Ordering::Relaxed);
        
        // Start metrics collection thread
        self.start_metrics_collector();
        
        // Start web server
        self.start_web_server()?;
        
        Ok(())
    }
    
    /// Stop the dashboard server
    pub fn stop(&self) {
        self.running.store(false, std::sync::atomic::Ordering::Relaxed);
    }
    
    /// Start the metrics collection thread
    fn start_metrics_collector(&self) {
        let history = Arc::clone(&self.metrics_history);
        let interval = self.config.update_interval;
        let running = Arc::clone(&self.running);
        
        thread::spawn(move || {
            while running.load(std::sync::atomic::Ordering::Relaxed) {
                // Collect current metrics
                let snapshot = SYSTEM_MONITOR.snapshot();
                
                // Add to history
                if let Ok(mut hist) = history.write() {
                    hist.add_snapshot(snapshot);
                }
                
                // Sleep until next collection
                thread::sleep(Duration::from_secs(interval));
            }
        });
    }
    
    /// Start the web server
    fn start_web_server(&self) -> Result<(), DashboardError> {
        // In a real implementation, this would start an HTTP server
        // For now, we'll just create the HTML template
        Ok(())
    }
    
    /// Get current metrics as JSON
    pub fn get_metrics_json(&self) -> String {
        let snapshot = SYSTEM_MONITOR.snapshot();
        serde_json::to_string_pretty(&snapshot).unwrap_or_default()
    }
    
    /// Get metrics history as JSON
    pub fn get_history_json(&self) -> String {
        if let Ok(history) = self.metrics_history.read() {
            serde_json::to_string_pretty(&*history).unwrap_or_default()
        } else {
            "{}".to_string()
        }
    }
    
    /// Generate HTML dashboard
    pub fn generate_html(&self) -> String {
        format!(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Neo-RS Monitoring Dashboard</title>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}
        
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: #333;
            min-height: 100vh;
            padding: 20px;
        }}
        
        .container {{
            max-width: 1400px;
            margin: 0 auto;
        }}
        
        h1 {{
            color: white;
            text-align: center;
            margin-bottom: 30px;
            font-size: 2.5em;
            text-shadow: 2px 2px 4px rgba(0,0,0,0.1);
        }}
        
        .metrics-grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
            gap: 20px;
            margin-bottom: 30px;
        }}
        
        .metric-card {{
            background: white;
            border-radius: 10px;
            padding: 20px;
            box-shadow: 0 10px 30px rgba(0,0,0,0.1);
            transition: transform 0.3s ease;
        }}
        
        .metric-card:hover {{
            transform: translateY(-5px);
            box-shadow: 0 15px 40px rgba(0,0,0,0.15);
        }}
        
        .metric-title {{
            font-size: 1.2em;
            font-weight: 600;
            color: #667eea;
            margin-bottom: 15px;
            display: flex;
            align-items: center;
        }}
        
        .metric-value {{
            font-size: 2em;
            font-weight: bold;
            color: #333;
            margin-bottom: 10px;
        }}
        
        .metric-label {{
            color: #666;
            font-size: 0.9em;
            margin-bottom: 5px;
        }}
        
        .metric-item {{
            display: flex;
            justify-content: space-between;
            padding: 8px 0;
            border-bottom: 1px solid #f0f0f0;
        }}
        
        .metric-item:last-child {{
            border-bottom: none;
        }}
        
        .status-indicator {{
            display: inline-block;
            width: 10px;
            height: 10px;
            border-radius: 50%;
            margin-right: 10px;
        }}
        
        .status-good {{ background: #10b981; }}
        .status-warning {{ background: #f59e0b; }}
        .status-error {{ background: #ef4444; }}
        
        .chart-container {{
            background: white;
            border-radius: 10px;
            padding: 20px;
            box-shadow: 0 10px 30px rgba(0,0,0,0.1);
            margin-bottom: 20px;
        }}
        
        .refresh-btn {{
            position: fixed;
            bottom: 30px;
            right: 30px;
            background: white;
            color: #667eea;
            border: none;
            padding: 15px 30px;
            border-radius: 50px;
            font-size: 1em;
            font-weight: 600;
            cursor: pointer;
            box-shadow: 0 10px 30px rgba(0,0,0,0.1);
            transition: all 0.3s ease;
        }}
        
        .refresh-btn:hover {{
            background: #667eea;
            color: white;
            transform: scale(1.05);
        }}
        
        @keyframes pulse {{
            0% {{ opacity: 1; }}
            50% {{ opacity: 0.5; }}
            100% {{ opacity: 1; }}
        }}
        
        .updating {{
            animation: pulse 1s infinite;
        }}
    </style>
</head>
<body>
    <div class="container">
        <h1>🚀 Neo-RS Monitoring Dashboard</h1>
        
        <div class="metrics-grid">
            <!-- Transactions Card -->
            <div class="metric-card">
                <div class="metric-title">
                    <span class="status-indicator status-good"></span>
                    Transactions
                </div>
                <div class="metric-value" id="tx-total">0</div>
                <div class="metric-label">Total Transactions</div>
                <div class="metric-item">
                    <span>Verified</span>
                    <span id="tx-verified">0</span>
                </div>
                <div class="metric-item">
                    <span>Failed</span>
                    <span id="tx-failed">0</span>
                </div>
                <div class="metric-item">
                    <span>Avg Verification</span>
                    <span id="tx-avg-time">0 μs</span>
                </div>
                <div class="metric-item">
                    <span>Mempool Size</span>
                    <span id="tx-mempool">0</span>
                </div>
            </div>
            
            <!-- Blocks Card -->
            <div class="metric-card">
                <div class="metric-title">
                    <span class="status-indicator status-good"></span>
                    Blocks
                </div>
                <div class="metric-value" id="block-height">0</div>
                <div class="metric-label">Current Height</div>
                <div class="metric-item">
                    <span>Total Blocks</span>
                    <span id="block-total">0</span>
                </div>
                <div class="metric-item">
                    <span>Avg Block Time</span>
                    <span id="block-time">0 ms</span>
                </div>
                <div class="metric-item">
                    <span>Avg Block Size</span>
                    <span id="block-size">0 B</span>
                </div>
                <div class="metric-item">
                    <span>Avg TX/Block</span>
                    <span id="block-tx">0</span>
                </div>
            </div>
            
            <!-- Network Card -->
            <div class="metric-card">
                <div class="metric-title">
                    <span class="status-indicator status-good"></span>
                    Network
                </div>
                <div class="metric-value" id="net-peers">0</div>
                <div class="metric-label">Connected Peers</div>
                <div class="metric-item">
                    <span>Messages Sent</span>
                    <span id="net-sent">0</span>
                </div>
                <div class="metric-item">
                    <span>Messages Received</span>
                    <span id="net-received">0</span>
                </div>
                <div class="metric-item">
                    <span>Avg Latency</span>
                    <span id="net-latency">0 ms</span>
                </div>
                <div class="metric-item">
                    <span>Connection Failures</span>
                    <span id="net-failures">0</span>
                </div>
            </div>
            
            <!-- VM Card -->
            <div class="metric-card">
                <div class="metric-title">
                    <span class="status-indicator status-good"></span>
                    Virtual Machine
                </div>
                <div class="metric-value" id="vm-executions">0</div>
                <div class="metric-label">Total Executions</div>
                <div class="metric-item">
                    <span>Successful</span>
                    <span id="vm-success">0</span>
                </div>
                <div class="metric-item">
                    <span>Failed</span>
                    <span id="vm-failed">0</span>
                </div>
                <div class="metric-item">
                    <span>Gas Consumed</span>
                    <span id="vm-gas">0</span>
                </div>
                <div class="metric-item">
                    <span>Avg Execution</span>
                    <span id="vm-time">0 μs</span>
                </div>
            </div>
            
            <!-- Consensus Card -->
            <div class="metric-card">
                <div class="metric-title">
                    <span class="status-indicator status-good"></span>
                    Consensus
                </div>
                <div class="metric-value" id="consensus-proposed">0</div>
                <div class="metric-label">Blocks Proposed</div>
                <div class="metric-item">
                    <span>Accepted</span>
                    <span id="consensus-accepted">0</span>
                </div>
                <div class="metric-item">
                    <span>Rejected</span>
                    <span id="consensus-rejected">0</span>
                </div>
                <div class="metric-item">
                    <span>View Changes</span>
                    <span id="consensus-views">0</span>
                </div>
                <div class="metric-item">
                    <span>Avg Consensus Time</span>
                    <span id="consensus-time">0 ms</span>
                </div>
            </div>
            
            <!-- Storage Card -->
            <div class="metric-card">
                <div class="metric-title">
                    <span class="status-indicator status-good"></span>
                    Storage
                </div>
                <div class="metric-value" id="storage-ops">0</div>
                <div class="metric-label">Total Operations</div>
                <div class="metric-item">
                    <span>Reads</span>
                    <span id="storage-reads">0</span>
                </div>
                <div class="metric-item">
                    <span>Writes</span>
                    <span id="storage-writes">0</span>
                </div>
                <div class="metric-item">
                    <span>Cache Hit Rate</span>
                    <span id="storage-cache">0%</span>
                </div>
                <div class="metric-item">
                    <span>Disk Usage</span>
                    <span id="storage-disk">0 MB</span>
                </div>
            </div>
            
            <!-- Errors Card -->
            <div class="metric-card">
                <div class="metric-title">
                    <span class="status-indicator" id="error-status"></span>
                    Errors & Warnings
                </div>
                <div class="metric-value" id="error-total">0</div>
                <div class="metric-label">Total Errors</div>
                <div class="metric-item">
                    <span>Critical</span>
                    <span id="error-critical">0</span>
                </div>
                <div class="metric-item">
                    <span>Warnings</span>
                    <span id="error-warnings">0</span>
                </div>
            </div>
            
            <!-- Performance Card -->
            <div class="metric-card">
                <div class="metric-title">
                    <span class="status-indicator status-good"></span>
                    Performance
                </div>
                <div class="metric-value" id="perf-cpu">0%</div>
                <div class="metric-label">CPU Usage</div>
                <div class="metric-item">
                    <span>Memory Usage</span>
                    <span id="perf-memory">0 MB</span>
                </div>
                <div class="metric-item">
                    <span>Thread Count</span>
                    <span id="perf-threads">0</span>
                </div>
                <div class="metric-item">
                    <span>GC Collections</span>
                    <span id="perf-gc">0</span>
                </div>
                <div class="metric-item">
                    <span>GC Pause Time</span>
                    <span id="perf-gc-time">0 ms</span>
                </div>
            </div>
        </div>
        
        <button class="refresh-btn" onclick="refreshMetrics()">
            🔄 Refresh
        </button>
    </div>
    
    <script>
        // Auto-refresh every second
        setInterval(refreshMetrics, 1000);
        
        function refreshMetrics() {{
            // Add updating animation
            document.querySelectorAll('.metric-card').forEach(card => {{
                card.classList.add('updating');
            }});
            
            // Fetch metrics from server
            fetch('/api/metrics')
                .then(response => response.json())
                .then(data => {{
                    updateDashboard(data);
                    
                    // Remove updating animation
                    setTimeout(() => {{
                        document.querySelectorAll('.metric-card').forEach(card => {{
                            card.classList.remove('updating');
                        }});
                    }}, 300);
                }})
                .catch(error => {{
                    console.error('Error fetching metrics:', error);
                }});
        }}
        
        function updateDashboard(data) {{
            // Update Transactions
            document.getElementById('tx-total').textContent = formatNumber(data.transactions.total_count);
            document.getElementById('tx-verified').textContent = formatNumber(data.transactions.verified_count);
            document.getElementById('tx-failed').textContent = formatNumber(data.transactions.failed_count);
            document.getElementById('tx-avg-time').textContent = formatNumber(data.transactions.average_verification_time_us) + ' μs';
            document.getElementById('tx-mempool').textContent = formatNumber(data.transactions.mempool_size);
            
            // Update Blocks
            document.getElementById('block-height').textContent = formatNumber(data.blocks.current_height);
            document.getElementById('block-total').textContent = formatNumber(data.blocks.total_count);
            document.getElementById('block-time').textContent = formatNumber(data.blocks.average_block_time_ms) + ' ms';
            document.getElementById('block-size').textContent = formatBytes(data.blocks.average_block_size_bytes);
            document.getElementById('block-tx').textContent = formatNumber(data.blocks.average_tx_per_block);
            
            // Update Network
            document.getElementById('net-peers').textContent = formatNumber(data.network.peer_count);
            document.getElementById('net-sent').textContent = formatNumber(data.network.messages_sent);
            document.getElementById('net-received').textContent = formatNumber(data.network.messages_received);
            document.getElementById('net-latency').textContent = formatNumber(data.network.average_latency_ms) + ' ms';
            document.getElementById('net-failures').textContent = formatNumber(data.network.connection_failures);
            
            // Update VM
            document.getElementById('vm-executions').textContent = formatNumber(data.vm.executions);
            document.getElementById('vm-success').textContent = formatNumber(data.vm.successful_executions);
            document.getElementById('vm-failed').textContent = formatNumber(data.vm.failed_executions);
            document.getElementById('vm-gas').textContent = formatNumber(data.vm.total_gas_consumed);
            document.getElementById('vm-time').textContent = formatNumber(data.vm.average_execution_time_us) + ' μs';
            
            // Update Consensus
            document.getElementById('consensus-proposed').textContent = formatNumber(data.consensus.blocks_proposed);
            document.getElementById('consensus-accepted').textContent = formatNumber(data.consensus.blocks_accepted);
            document.getElementById('consensus-rejected').textContent = formatNumber(data.consensus.blocks_rejected);
            document.getElementById('consensus-views').textContent = formatNumber(data.consensus.view_changes);
            document.getElementById('consensus-time').textContent = formatNumber(data.consensus.average_consensus_time_ms) + ' ms';
            
            // Update Storage
            const totalOps = data.storage.reads + data.storage.writes + data.storage.deletes;
            document.getElementById('storage-ops').textContent = formatNumber(totalOps);
            document.getElementById('storage-reads').textContent = formatNumber(data.storage.reads);
            document.getElementById('storage-writes').textContent = formatNumber(data.storage.writes);
            const cacheRate = data.storage.reads > 0 ? 
                Math.round((data.storage.cache_hits / data.storage.reads) * 100) : 0;
            document.getElementById('storage-cache').textContent = cacheRate + '%';
            document.getElementById('storage-disk').textContent = formatBytes(data.storage.disk_usage_bytes);
            
            // Update Errors
            document.getElementById('error-total').textContent = formatNumber(data.errors.total_errors);
            document.getElementById('error-critical').textContent = formatNumber(data.errors.critical_errors);
            document.getElementById('error-warnings').textContent = formatNumber(data.errors.warnings);
            
            // Update error status indicator
            const errorStatus = document.getElementById('error-status');
            if (data.errors.critical_errors > 0) {{
                errorStatus.className = 'status-indicator status-error';
            }} else if (data.errors.warnings > 10) {{
                errorStatus.className = 'status-indicator status-warning';
            }} else {{
                errorStatus.className = 'status-indicator status-good';
            }}
            
            // Update Performance
            document.getElementById('perf-cpu').textContent = data.performance.cpu_usage_percent + '%';
            document.getElementById('perf-memory').textContent = formatBytes(data.performance.memory_usage_bytes);
            document.getElementById('perf-threads').textContent = formatNumber(data.performance.thread_count);
            document.getElementById('perf-gc').textContent = formatNumber(data.performance.gc_collections);
            document.getElementById('perf-gc-time').textContent = formatNumber(data.performance.gc_pause_time_ms) + ' ms';
        }}
        
        function formatNumber(num) {{
            if (num >= 1000000) {{
                return (num / 1000000).toFixed(1) + 'M';
            }} else if (num >= 1000) {{
                return (num / 1000).toFixed(1) + 'K';
            }}
            return num.toString();
        }}
        
        function formatBytes(bytes) {{
            if (bytes >= 1073741824) {{
                return (bytes / 1073741824).toFixed(1) + ' GB';
            }} else if (bytes >= 1048576) {{
                return (bytes / 1048576).toFixed(1) + ' MB';
            }} else if (bytes >= 1024) {{
                return (bytes / 1024).toFixed(1) + ' KB';
            }}
            return bytes + ' B';
        }}
        
        // Initial load
        refreshMetrics();
    </script>
</body>
</html>
        "#)
    }
}

/// Metrics history storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsHistory {
    max_size: usize,
    snapshots: VecDeque<SystemMetricsSnapshot>,
}

impl MetricsHistory {
    pub fn new(max_size: usize) -> Self {
        Self {
            max_size,
            snapshots: VecDeque::with_capacity(max_size),
        }
    }
    
    pub fn add_snapshot(&mut self, snapshot: SystemMetricsSnapshot) {
        if self.snapshots.len() >= self.max_size {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(snapshot);
    }
    
    pub fn get_latest(&self) -> Option<&SystemMetricsSnapshot> {
        self.snapshots.back()
    }
    
    pub fn get_history(&self) -> &VecDeque<SystemMetricsSnapshot> {
        &self.snapshots
    }
}

/// Dashboard error types
#[derive(Debug)]
pub enum DashboardError {
    AlreadyRunning,
    ServerError(String),
    IoError(std::io::Error),
}

impl std::fmt::Display for DashboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning => write!(f, "Dashboard is already running"),
            Self::ServerError(msg) => write!(f, "Server error: {}", msg),
            Self::IoError(err) => write!(f, "IO error: {}", err),
        }
    }
}

impl std::error::Error for DashboardError {}

impl From<std::io::Error> for DashboardError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}