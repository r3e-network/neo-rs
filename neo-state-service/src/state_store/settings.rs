use neo_primitives::UnhandledExceptionPolicy;

/// Settings for the state service.
#[derive(Debug, Clone)]
pub struct StateServiceSettings {
    /// Whether to maintain full state history.
    pub full_state: bool,
    /// Path to the state store database.
    pub path: String,
    /// Network magic number (used for config validation and path formatting).
    pub network: u32,
    /// Whether to auto-start state root verification when a wallet is available.
    pub auto_verify: bool,
    /// Maximum number of results returned by findstates.
    pub max_find_result_items: usize,
    /// Policy for handling unhandled exceptions.
    pub exception_policy: UnhandledExceptionPolicy,
}

impl Default for StateServiceSettings {
    fn default() -> Self {
        Self {
            full_state: false,
            path: "Data_MPT_{0}".to_string(),
            network: 5195086,
            auto_verify: false,
            max_find_result_items: 100,
            exception_policy: UnhandledExceptionPolicy::StopPlugin,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_exception_policy_remains_stop_plugin_without_auto_verify() {
        let settings = StateServiceSettings::default();
        let policy: neo_primitives::UnhandledExceptionPolicy = settings.exception_policy;

        assert_eq!(policy, neo_primitives::UnhandledExceptionPolicy::StopPlugin);
        assert!(!settings.auto_verify);
        assert!(!settings.full_state);
        assert_eq!(settings.path, "Data_MPT_{0}");
        assert_eq!(settings.network, 5195086);
        assert_eq!(settings.max_find_result_items, 100);
    }
}
