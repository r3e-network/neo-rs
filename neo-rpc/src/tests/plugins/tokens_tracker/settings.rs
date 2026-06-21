use super::*;
use serde_json::json;

#[test]
fn default_settings_enable_standard_nep_trackers() {
    let settings = TokensTrackerSettings::default();

    assert_eq!(
        settings.enabled_trackers,
        TokensTrackerSettings::default_enabled_trackers()
    );
    assert!(settings.enabled_nep17());
    assert!(settings.enabled_nep11());
}

#[test]
fn from_config_defaults_empty_tracker_list_to_standard_nep_trackers() {
    for config in [
        json!({}),
        json!({ "EnabledTrackers": [] }),
        json!({ "EnabledTrackers": ["", "  "] }),
        json!({ "PluginConfiguration": { "EnabledTrackers": ["", "  "] } }),
    ] {
        let settings = TokensTrackerSettings::from_config(&config);
        assert_eq!(
            settings.enabled_trackers,
            TokensTrackerSettings::default_enabled_trackers()
        );
    }
}

#[test]
fn from_config_normalizes_explicit_tracker_list() {
    let settings = TokensTrackerSettings::from_config(&json!({
        "EnabledTrackers": [" nep-17 ", "", "Nep-11", "NEP-17", "nep-11"],
    }));

    assert_eq!(
        settings.enabled_trackers,
        vec!["NEP-17".to_string(), "NEP-11".to_string()]
    );
    assert!(settings.enabled_nep17());
    assert!(settings.enabled_nep11());
}

#[test]
fn enabled_tracker_checks_accept_known_standards_case_insensitively() {
    let mut settings = TokensTrackerSettings::default();
    settings.enabled_trackers = vec!["nep-17".to_string(), "nep-11".to_string()];

    assert!(settings.enabled_nep17());
    assert!(settings.enabled_nep11());
}

#[test]
fn from_config_accepts_u32_boundary_integer_fields() {
    let settings = TokensTrackerSettings::from_config(&json!({
        "MaxResults": u32::MAX as u64,
        "Network": u32::MAX as u64,
    }));

    assert_eq!(settings.max_results, u32::MAX);
    assert_eq!(settings.network, u32::MAX);
    assert_eq!(
        settings.max_results_limit(),
        usize::try_from(u32::MAX).unwrap_or(usize::MAX)
    );
}

#[test]
fn from_config_ignores_out_of_range_integer_fields_without_truncating() {
    let defaults = TokensTrackerSettings::default();
    let settings = TokensTrackerSettings::from_config(&json!({
        "MaxResults": u32::MAX as u64 + 1,
        "Network": u32::MAX as u64 + 1,
    }));

    assert_eq!(settings.max_results, defaults.max_results);
    assert_eq!(settings.network, defaults.network);
}

#[test]
fn from_config_uses_shared_exception_policy_parser() {
    let settings = TokensTrackerSettings::from_config(&json!({
        "UnhandledExceptionPolicy": " stopplugin ",
    }));
    assert_eq!(
        settings.exception_policy,
        UnhandledExceptionPolicy::StopPlugin
    );

    let settings = TokensTrackerSettings::from_config(&json!({
        "UnhandledExceptionPolicy": "unknown",
    }));
    assert_eq!(
        settings.exception_policy,
        UnhandledExceptionPolicy::StopNode
    );
}
