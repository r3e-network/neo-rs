use super::*;
use neo_primitives::unhandled_exception_policy::UnhandledExceptionPolicy;

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
