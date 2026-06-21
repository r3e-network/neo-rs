//! TokensTracker settings.
//!
//! Configuration for the tokens tracker including enabled standards,
//! database path, and history tracking options.

use serde::Deserialize;
use serde_json::Value;

use neo_primitives::unhandled_exception_policy::UnhandledExceptionPolicy;

/// Default token tracker standards enabled by Neo's TokensTracker plugin.
pub const DEFAULT_ENABLED_TRACKER_STANDARDS: [&str; 2] = ["NEP-17", "NEP-11"];

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
            db_path: "TokenBalanceData".to_string(),
            track_history: true,
            max_results: 1000,
            network: 860_833_102,
            enabled_trackers: Self::default_enabled_trackers(),
            exception_policy: UnhandledExceptionPolicy::StopNode,
        }
    }
}

impl TokensTrackerSettings {
    /// Returns the default NEP standards tracked when no explicit list is
    /// configured.
    pub fn default_enabled_trackers() -> Vec<String> {
        DEFAULT_ENABLED_TRACKER_STANDARDS
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    /// Trims and filters configured tracker names, falling back to the standard
    /// NEP-17/NEP-11 pair when no non-empty tracker names are supplied.
    pub fn normalize_enabled_trackers<S>(trackers: impl IntoIterator<Item = S>) -> Vec<String>
    where
        S: AsRef<str>,
    {
        let mut normalized = trackers
            .into_iter()
            .map(|tracker| normalize_tracker_name(tracker.as_ref()))
            .filter(|tracker| !tracker.is_empty())
            .collect::<Vec<_>>();
        deduplicate_tracker_names(&mut normalized);
        if normalized.is_empty() {
            Self::default_enabled_trackers()
        } else {
            normalized
        }
    }

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

        if let Some(max_results) = optional_u32_field(section, "MaxResults") {
            settings.max_results = max_results;
        }

        if let Some(network) = optional_u32_field(section, "Network") {
            settings.network = network;
        }

        if let Some(trackers) = section.get("EnabledTrackers").and_then(|v| v.as_array()) {
            settings.enabled_trackers =
                Self::normalize_enabled_trackers(trackers.iter().filter_map(|v| v.as_str()));
        }

        if let Some(policy) = section
            .get("UnhandledExceptionPolicy")
            .and_then(|v| v.as_str())
        {
            settings.exception_policy = policy.parse().unwrap_or_default();
        }

        settings
    }

    /// Returns true if NEP-11 tracking is enabled.
    pub fn enabled_nep11(&self) -> bool {
        self.enabled_trackers
            .iter()
            .any(|tracker| tracker.eq_ignore_ascii_case("NEP-11"))
    }

    /// Returns true if NEP-17 tracking is enabled.
    pub fn enabled_nep17(&self) -> bool {
        self.enabled_trackers
            .iter()
            .any(|tracker| tracker.eq_ignore_ascii_case("NEP-17"))
    }

    /// Returns `max_results` in the host index type used by RPC collection
    /// helpers.
    pub fn max_results_limit(&self) -> usize {
        usize::try_from(self.max_results).unwrap_or(usize::MAX)
    }
}

fn normalize_tracker_name(tracker: &str) -> String {
    let tracker = tracker.trim();
    for standard in DEFAULT_ENABLED_TRACKER_STANDARDS {
        if tracker.eq_ignore_ascii_case(standard) {
            return standard.to_string();
        }
    }
    tracker.to_string()
}

fn deduplicate_tracker_names(trackers: &mut Vec<String>) {
    let mut unique = Vec::with_capacity(trackers.len());
    for tracker in trackers.drain(..) {
        if !unique
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&tracker))
        {
            unique.push(tracker);
        }
    }
    *trackers = unique;
}

fn optional_u32_field(section: &Value, field: &str) -> Option<u32> {
    section
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

#[cfg(test)]
#[path = "../../tests/plugins/tokens_tracker/settings.rs"]
mod tests;
