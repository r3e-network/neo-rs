use super::*;
use neo_primitives::unhandled_exception_policy::UnhandledExceptionPolicy;

#[test]
fn token_tracker_engine_paths_use_explicit_native_provider() {
    let engine_sources = [
        (
            "NEP-17 tracker",
            include_str!("../../../plugins/tokens_tracker/trackers/nep_17/nep17_tracker.rs"),
        ),
        (
            "NEP-11 tracker",
            include_str!("../../../plugins/tokens_tracker/trackers/nep_11/nep11_tracker.rs"),
        ),
    ];

    for (name, source) in engine_sources {
        assert!(
            source.contains("new_with_shared_block_and_native_contract_provider"),
            "{name} should construct ApplicationEngine with an explicit native provider"
        );
        assert!(
            source.contains("native_contract_provider"),
            "{name} should use the provider captured by the tracker runtime"
        );
        assert!(
            !source.contains("ApplicationEngine::new("),
            "{name} should not read the ambient native-provider bridge"
        );
    }

    let runtime = include_str!("../../../plugins/tokens_tracker/runtime.rs");
    assert!(
        runtime.contains("native_contract_provider"),
        "TokensTracker runtime should thread the composed native provider into concrete trackers"
    );
}

fn tracker_with_policy(exception_policy: UnhandledExceptionPolicy) -> TokensTracker {
    TokensTracker {
        settings: TokensTrackerSettings {
            exception_policy,
            ..TokensTrackerSettings::default()
        },
        trackers: RwLock::new(Vec::new()),
        disabled: AtomicBool::new(false),
    }
}

#[test]
fn result_action_disables_tracker_when_commit_error_stops_plugin() {
    let tracker = tracker_with_policy(UnhandledExceptionPolicy::StopPlugin);

    let should_continue = tracker.run_tracker_result_action("test", "commit", || {
        Err(neo_error::CoreError::other("injected commit failure"))
    });

    assert!(!should_continue);
    assert!(tracker.disabled.load(Ordering::Relaxed));
}

#[test]
fn result_action_keeps_tracker_enabled_when_commit_error_continues() {
    let tracker = tracker_with_policy(UnhandledExceptionPolicy::Continue);

    let should_continue = tracker.run_tracker_result_action("test", "commit", || {
        Err(neo_error::CoreError::other("injected commit failure"))
    });

    assert!(should_continue);
    assert!(!tracker.disabled.load(Ordering::Relaxed));
}
