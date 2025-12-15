//! TokensTracker settings.
//!
//! Configuration for the tokens tracker including enabled standards,
//! database path, and history tracking options.

use serde::Deserialize;
use serde_json::Value;

/// Exception handling policy for the tokens tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum UnhandledExceptionPolicy {
    /// Ignore exceptions and continue processing.
    Ignore,
    /// Stop the plugin/tracker on exception.
    StopPlugin,
    /// Stop the node on exception.
    #[default]
    StopNode,
    /// Continue processing after logging exception.
    Continue,
    /// Terminate the process immediately.
    Terminate,
}

/// Configuration settings for the TokensTracker.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TokensTrackerSettings {
    /// Path to the token balance database.
    pub db_path: String,
    /// Whether to track transfer history (not just balances).
    pub track_history: bool,
    /// Maximum results to return in RPC queries.
    pub max_results: u32,
    /// Network ID this tracker is configured for.
    pub network: u32,
    /// List of enabled tracker standards (e.g., "NEP-11", "NEP-17").
    pub enabled_trackers: Vec<String>,
    /// Policy for handling unhandled exceptions.
    pub exception_policy: UnhandledExceptionPolicy,
}

impl Default for TokensTrackerSettings {
    fn default() -> Self {
        Self {
            db_path: "TokensBalanceData".to_string(),
            track_history: true,
            max_results: 1000,
            network: 860_833_102,
            enabled_trackers: Vec::new(),
            exception_policy: UnhandledExceptionPolicy::StopNode,
        }
    }
}

impl TokensTrackerSettings {
    /// Creates settings from a JSON configuration value.
    pub fn from_config(value: &Value) -> Self {
        let section = value.get("PluginConfiguration").unwrap_or(value);
        let mut settings = TokensTrackerSettings::default();

        if let Some(db_path) = section.get("DBPath").and_then(|v| v.as_str()) {
            if !db_path.trim().is_empty() {
                settings.db_path = db_path.trim().to_string();
            }
        }

        if let Some(track_history) = section.get("TrackHistory").and_then(|v| v.as_bool()) {
            settings.track_history = track_history;
        }

        if let Some(max_results) = section.get("MaxResults").and_then(|v| v.as_u64()) {
            settings.max_results = max_results as u32;
        }

        if let Some(network) = section.get("Network").and_then(|v| v.as_u64()) {
            settings.network = network as u32;
        }

        if let Some(trackers) = section.get("EnabledTrackers").and_then(|v| v.as_array()) {
            settings.enabled_trackers = trackers
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
        }

        if let Some(policy) = section
            .get("UnhandledExceptionPolicy")
            .and_then(|v| v.as_str())
        {
            settings.exception_policy = match policy.to_ascii_lowercase().as_str() {
                "ignore" => UnhandledExceptionPolicy::Ignore,
                "stopplugin" => UnhandledExceptionPolicy::StopPlugin,
                "continue" => UnhandledExceptionPolicy::Continue,
                "terminate" => UnhandledExceptionPolicy::Terminate,
                _ => UnhandledExceptionPolicy::StopNode,
            };
        }

        settings
    }

    /// Returns true if NEP-11 tracking is enabled.
    pub fn enabled_nep11(&self) -> bool {
        self.enabled_trackers.iter().any(|s| s == "NEP-11")
    }

    /// Returns true if NEP-17 tracking is enabled.
    pub fn enabled_nep17(&self) -> bool {
        self.enabled_trackers.iter().any(|s| s == "NEP-17")
    }
}
