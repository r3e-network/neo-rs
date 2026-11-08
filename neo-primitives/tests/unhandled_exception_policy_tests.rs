use neo_primitives::{UnhandledExceptionPolicy, panic_message};
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn unhandled_exception_policy_ignore_continues() {
    let stopped = AtomicBool::new(false);
    let result = UnhandledExceptionPolicy::Ignore.apply(|| {
        stopped.store(true, Ordering::Relaxed);
    });

    assert!(result);
    assert!(!stopped.load(Ordering::Relaxed));
}

#[test]
fn unhandled_exception_policy_continue_continues() {
    let stopped = AtomicBool::new(false);
    let result = UnhandledExceptionPolicy::Continue.apply(|| {
        stopped.store(true, Ordering::Relaxed);
    });

    assert!(result);
    assert!(!stopped.load(Ordering::Relaxed));
}

#[test]
fn unhandled_exception_policy_stop_plugin_invokes_callback() {
    let stopped = AtomicBool::new(false);
    let result = UnhandledExceptionPolicy::StopPlugin.apply(|| {
        stopped.store(true, Ordering::Relaxed);
    });

    assert!(!result);
    assert!(stopped.load(Ordering::Relaxed));
}

#[test]
fn unhandled_exception_policy_default_is_stop_node() {
    let default = UnhandledExceptionPolicy::default();
    assert_eq!(default, UnhandledExceptionPolicy::StopNode);
}

#[test]
fn unhandled_exception_policy_clone() {
    let policy = UnhandledExceptionPolicy::Ignore;
    let cloned = policy.clone();
    assert_eq!(policy, cloned);
}

#[test]
fn unhandled_exception_policy_copy() {
    let policy = UnhandledExceptionPolicy::Continue;
    let copied = policy;
    assert_eq!(policy, copied);
}

#[test]
fn unhandled_exception_policy_debug_format() {
    assert_eq!(format!("{:?}", UnhandledExceptionPolicy::Ignore), "Ignore");
    assert_eq!(format!("{:?}", UnhandledExceptionPolicy::StopPlugin), "StopPlugin");
    assert_eq!(format!("{:?}", UnhandledExceptionPolicy::StopNode), "StopNode");
    assert_eq!(format!("{:?}", UnhandledExceptionPolicy::Continue), "Continue");
    assert_eq!(format!("{:?}", UnhandledExceptionPolicy::Terminate), "Terminate");
}

#[test]
fn unhandled_exception_policy_equality() {
    assert_eq!(UnhandledExceptionPolicy::Ignore, UnhandledExceptionPolicy::Ignore);
    assert_ne!(UnhandledExceptionPolicy::Ignore, UnhandledExceptionPolicy::Continue);
}

#[test]
fn unhandled_exception_policy_serde_deserialization() {
    let policy: UnhandledExceptionPolicy = serde_json::from_str("\"Ignore\"").unwrap();
    assert_eq!(policy, UnhandledExceptionPolicy::Ignore);

    let policy: UnhandledExceptionPolicy = serde_json::from_str("\"StopPlugin\"").unwrap();
    assert_eq!(policy, UnhandledExceptionPolicy::StopPlugin);

    let policy: UnhandledExceptionPolicy = serde_json::from_str("\"StopNode\"").unwrap();
    assert_eq!(policy, UnhandledExceptionPolicy::StopNode);

    let policy: UnhandledExceptionPolicy = serde_json::from_str("\"Continue\"").unwrap();
    assert_eq!(policy, UnhandledExceptionPolicy::Continue);

    let policy: UnhandledExceptionPolicy = serde_json::from_str("\"Terminate\"").unwrap();
    assert_eq!(policy, UnhandledExceptionPolicy::Terminate);
}

#[test]
fn panic_message_extracts_str_payload() {
    let payload: Box<dyn std::any::Any + Send> = Box::new("panic message");
    let message = panic_message(payload.as_ref(), "fallback");
    assert_eq!(message, "panic message");
}

#[test]
fn panic_message_extracts_string_payload() {
    let payload: Box<dyn std::any::Any + Send> = Box::new("owned panic".to_string());
    let message = panic_message(payload.as_ref(), "fallback");
    assert_eq!(message, "owned panic");
}

#[test]
fn panic_message_uses_fallback_for_unknown_payload() {
    let payload: Box<dyn std::any::Any + Send> = Box::new(42u32);
    let message = panic_message(payload.as_ref(), "custom fallback");
    assert_eq!(message, "custom fallback");
}

#[test]
fn panic_message_uses_fallback_for_non_string_types() {
    let payload: Box<dyn std::any::Any + Send> = Box::new(vec![1, 2, 3]);
    let message = panic_message(payload.as_ref(), "fallback text");
    assert_eq!(message, "fallback text");
}

#[test]
fn panic_message_empty_string_payload() {
    let payload: Box<dyn std::any::Any + Send> = Box::new("");
    let message = panic_message(payload.as_ref(), "fallback");
    assert_eq!(message, "");
}

#[test]
fn panic_message_empty_string_owned_payload() {
    let payload: Box<dyn std::any::Any + Send> = Box::new(String::new());
    let message = panic_message(payload.as_ref(), "fallback");
    assert_eq!(message, "");
}

#[test]
fn panic_message_multiline_payload() {
    let payload: Box<dyn std::any::Any + Send> = Box::new("line1\nline2\nline3");
    let message = panic_message(payload.as_ref(), "fallback");
    assert_eq!(message, "line1\nline2\nline3");
}

#[test]
fn panic_message_unicode_payload() {
    let payload: Box<dyn std::any::Any + Send> = Box::new("错误信息 🚨");
    let message = panic_message(payload.as_ref(), "fallback");
    assert_eq!(message, "错误信息 🚨");
}

#[test]
fn panic_message_different_fallbacks() {
    let payload: Box<dyn std::any::Any + Send> = Box::new(123);

    let message1 = panic_message(payload.as_ref(), "fallback 1");
    assert_eq!(message1, "fallback 1");

    let message2 = panic_message(payload.as_ref(), "fallback 2");
    assert_eq!(message2, "fallback 2");
}
