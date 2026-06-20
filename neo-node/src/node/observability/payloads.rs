//! Error, heartbeat, and provider-specific observability payload codecs.

use std::{any::Any, fmt, panic::PanicHookInfo};

use chrono::Utc;
use serde_json::{Value, json};

#[derive(Clone)]
pub(super) struct ObservabilityMetadata {
    pub(super) service_name: String,
    pub(super) environment: Option<String>,
    pub(super) node_id: Option<String>,
    pub(super) network: u32,
    pub(super) version: &'static str,
}

#[derive(Clone)]
pub(super) struct ErrorReport {
    pub(super) event_type: String,
    pub(super) message: String,
    pub(super) timestamp: String,
    pub(super) location: Option<ReportLocation>,
}

#[derive(Clone)]
pub(super) struct ReportLocation {
    pub(super) file_path: String,
    pub(super) line_number: u32,
    pub(super) column_number: u32,
}

impl ErrorReport {
    pub(super) fn startup(error: &anyhow::Error) -> Self {
        Self {
            event_type: "startup_error".to_string(),
            message: format!("{error:#}"),
            timestamp: Utc::now().to_rfc3339(),
            location: None,
        }
    }

    pub(super) fn from_panic(panic_info: &PanicHookInfo<'_>) -> Self {
        Self {
            event_type: "panic".to_string(),
            message: panic_message(panic_info),
            timestamp: Utc::now().to_rfc3339(),
            location: panic_info.location().map(|location| ReportLocation {
                file_path: location.file().to_string(),
                line_number: location.line(),
                column_number: location.column(),
            }),
        }
    }

    pub(super) fn heartbeat_failure(endpoint_name: &str, error: &anyhow::Error) -> Self {
        Self {
            event_type: "heartbeat_failure".to_string(),
            message: format!("heartbeat {endpoint_name} failed: {error:#}"),
            timestamp: Utc::now().to_rfc3339(),
            location: None,
        }
    }

    pub(super) fn runtime_error(component: &str, error: impl fmt::Display) -> Self {
        let component = component.trim();
        let component = if component.is_empty() {
            "runtime"
        } else {
            component
        };
        Self {
            event_type: "runtime_error".to_string(),
            message: format!("{component}: {error}"),
            timestamp: Utc::now().to_rfc3339(),
            location: None,
        }
    }

    pub(super) fn background_task_panic(task_name: &str, panic_payload: &(dyn Any + Send)) -> Self {
        let task_name = normalized_task_name(task_name);
        Self {
            event_type: "background_task_panic".to_string(),
            message: format!(
                "{task_name} panicked: {}",
                panic_payload_message(panic_payload)
            ),
            timestamp: Utc::now().to_rfc3339(),
            location: None,
        }
    }

    pub(super) fn background_task_exit(task_name: &str) -> Self {
        let task_name = normalized_task_name(task_name);
        Self {
            event_type: "background_task_exit".to_string(),
            message: format!("{task_name} exited unexpectedly"),
            timestamp: Utc::now().to_rfc3339(),
            location: None,
        }
    }

    pub(super) fn background_task_error(task_name: &str, error: &anyhow::Error) -> Self {
        let task_name = normalized_task_name(task_name);
        Self {
            event_type: "background_task_error".to_string(),
            message: format!("{task_name} failed: {error:#}"),
            timestamp: Utc::now().to_rfc3339(),
            location: None,
        }
    }
}

pub(super) fn build_generic_error_payload(
    metadata: &ObservabilityMetadata,
    report: &ErrorReport,
) -> Value {
    json!({
        "service": service_payload(metadata),
        "event": {
            "type": report.event_type,
            "message": report.message,
            "timestamp": report.timestamp,
            "location": report.location.as_ref().map(location_payload),
        }
    })
}

pub(super) fn build_better_stack_error_payload(
    metadata: &ObservabilityMetadata,
    report: &ErrorReport,
) -> Value {
    let mut payload = json!({
        "message": report.message,
        "dt": report.timestamp,
        "level": "error",
        "event_type": report.event_type,
        "service": metadata.service_name,
        "version": metadata.version,
        "environment": metadata.environment,
        "node_id": metadata.node_id,
        "network": format!("0x{:08X}", metadata.network),
    });
    if let Some(location) = &report.location {
        payload["location"] = location_payload(location);
    }
    payload
}

pub(super) fn build_sentry_error_payload(
    metadata: &ObservabilityMetadata,
    report: &ErrorReport,
) -> Value {
    let network = format!("0x{:08X}", metadata.network);
    json!({
        "timestamp": &report.timestamp,
        "platform": "rust",
        "level": sentry_error_level(report),
        "logger": "neo-node",
        "transaction": &report.event_type,
        "message": &report.message,
        "release": format!("neo-node@{}", metadata.version),
        "environment": metadata.environment.as_deref(),
        "server_name": metadata.node_id.as_deref(),
        "tags": {
            "service": &metadata.service_name,
            "network": &network,
            "event_type": &report.event_type,
        },
        "extra": {
            "node_id": metadata.node_id.as_deref(),
            "network": &network,
        },
        "exception": {
            "values": [sentry_exception_payload(report)],
        },
    })
}

pub(super) fn build_google_error_payload(
    metadata: &ObservabilityMetadata,
    report: &ErrorReport,
) -> Value {
    let mut payload = json!({
        "serviceContext": {
            "service": metadata.service_name,
            "version": metadata.version,
        },
        "eventTime": report.timestamp,
        "message": format!("{}: {}", report.event_type, report.message),
    });

    let mut context = json!({
        "reportLocation": google_report_location(report),
    });
    if let Some(node_id) = metadata.node_id.as_deref() {
        context["user"] = json!(node_id);
    }
    payload["context"] = context;

    payload
}

fn google_report_location(report: &ErrorReport) -> Value {
    match &report.location {
        Some(location) => json!({
            "filePath": location.file_path,
            "lineNumber": location.line_number,
            "functionName": report.event_type,
        }),
        None => json!({
            "filePath": "neo-node",
            "lineNumber": 0,
            "functionName": report.event_type,
        }),
    }
}

fn sentry_exception_payload(report: &ErrorReport) -> Value {
    let mut exception = json!({
        "type": &report.event_type,
        "value": &report.message,
    });
    if let Some(location) = &report.location {
        exception["stacktrace"] = json!({
            "frames": [{
                "filename": &location.file_path,
                "lineno": location.line_number,
                "colno": location.column_number,
                "function": &report.event_type,
            }]
        });
    }
    exception
}

fn sentry_error_level(report: &ErrorReport) -> &'static str {
    match report.event_type.as_str() {
        "panic" | "background_task_panic" => "fatal",
        _ => "error",
    }
}

pub(super) fn build_heartbeat_payload(
    metadata: &ObservabilityMetadata,
    node_health: Option<Value>,
) -> Value {
    let mut payload = json!({
        "service": service_payload(metadata),
        "event": {
            "type": "heartbeat",
            "timestamp": Utc::now().to_rfc3339(),
        }
    });
    if let Some(node_health) = node_health {
        payload["node"] = node_health;
    }
    payload
}

pub(super) fn service_payload(metadata: &ObservabilityMetadata) -> Value {
    json!({
        "name": metadata.service_name,
        "version": metadata.version,
        "environment": metadata.environment,
        "node_id": metadata.node_id,
        "network": format!("0x{:08X}", metadata.network),
    })
}

fn location_payload(location: &ReportLocation) -> Value {
    json!({
        "file_path": location.file_path,
        "line_number": location.line_number,
        "column_number": location.column_number,
    })
}

fn panic_message(panic_info: &PanicHookInfo<'_>) -> String {
    panic_payload_message(panic_info.payload())
}

fn panic_payload_message(panic_payload: &(dyn Any + Send)) -> String {
    if let Some(message) = panic_payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = panic_payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "node panicked".to_string()
    }
}

fn normalized_task_name(task_name: &str) -> &str {
    let task_name = task_name.trim();
    if task_name.is_empty() {
        "background_task"
    } else {
        task_name
    }
}
