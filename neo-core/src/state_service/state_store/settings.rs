use crate::unhandled_exception_policy::UnhandledExceptionPolicy;

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
            network: 0,
            auto_verify: false,
            max_find_result_items: 100,
            exception_policy: UnhandledExceptionPolicy::StopPlugin,
        }
    }
}
