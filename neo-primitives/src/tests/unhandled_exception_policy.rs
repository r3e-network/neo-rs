use super::{UnhandledExceptionPolicy, panic_message};
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn continuing_policies_do_not_stop_plugin() {
    let stopped = AtomicBool::new(false);

    assert!(UnhandledExceptionPolicy::Ignore.apply(|| {
        stopped.store(true, Ordering::Relaxed);
    }));
    assert!(UnhandledExceptionPolicy::Continue.apply(|| {
        stopped.store(true, Ordering::Relaxed);
    }));

    assert!(!stopped.load(Ordering::Relaxed));
}

#[test]
fn stop_plugin_invokes_callback_and_stops_processing() {
    let stopped = AtomicBool::new(false);

    let should_continue = UnhandledExceptionPolicy::StopPlugin.apply(|| {
        stopped.store(true, Ordering::Relaxed);
    });

    assert!(!should_continue);
    assert!(stopped.load(Ordering::Relaxed));
}

#[test]
fn parses_policy_names_case_insensitively() {
    assert_eq!(
        " ignore ".parse::<UnhandledExceptionPolicy>(),
        Ok(UnhandledExceptionPolicy::Ignore)
    );
    assert_eq!(
        "STOPPLUGIN".parse::<UnhandledExceptionPolicy>(),
        Ok(UnhandledExceptionPolicy::StopPlugin)
    );
    assert_eq!(
        "StopNode".parse::<UnhandledExceptionPolicy>(),
        Ok(UnhandledExceptionPolicy::StopNode)
    );
    assert_eq!(
        "continue".parse::<UnhandledExceptionPolicy>(),
        Ok(UnhandledExceptionPolicy::Continue)
    );
    assert_eq!(
        "Terminate".parse::<UnhandledExceptionPolicy>(),
        Ok(UnhandledExceptionPolicy::Terminate)
    );
    assert!("missing".parse::<UnhandledExceptionPolicy>().is_err());
}

#[test]
fn panic_message_extracts_string_payloads() {
    let str_payload: Box<dyn std::any::Any + Send> = Box::new("borrowed panic");
    let string_payload: Box<dyn std::any::Any + Send> = Box::new("owned panic".to_string());

    assert_eq!(
        panic_message(str_payload.as_ref(), "fallback"),
        "borrowed panic"
    );
    assert_eq!(
        panic_message(string_payload.as_ref(), "fallback"),
        "owned panic"
    );
}

#[test]
fn panic_message_uses_caller_fallback_for_unknown_payloads() {
    let payload: Box<dyn std::any::Any + Send> = Box::new(7_u8);

    assert_eq!(panic_message(payload.as_ref(), "fallback"), "fallback");
}
