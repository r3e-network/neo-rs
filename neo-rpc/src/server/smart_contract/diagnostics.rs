//! Diagnostic JSON projection for smart-contract invoke responses.
//!
//! Invocation diagnostics are optional debug payloads. This module owns their
//! C#-compatible response shape so the shared helper bucket does not mix
//! diagnostic formatting with script, stack, and request utilities.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_execution::ApplicationEngine;
use serde_json::{Map, Value, json};

use crate::server::diagnostic::{Diagnostic, DiagnosticInvocation};

pub(super) fn diagnostic_invocation_to_json(diagnostic: &Diagnostic) -> Value {
    fn to_json_node(node: DiagnosticInvocation) -> Value {
        let mut obj = Map::new();
        obj.insert("hash".to_string(), Value::String(node.hash.to_string()));
        if !node.children.is_empty() {
            let children = node
                .children
                .into_iter()
                .map(to_json_node)
                .collect::<Vec<_>>();
            obj.insert("call".to_string(), Value::Array(children));
        }
        Value::Object(obj)
    }

    match diagnostic.invocation_root() {
        Some(root) => to_json_node(root),
        None => Value::Null,
    }
}

pub(super) fn diagnostic_storage_changes(engine: &ApplicationEngine) -> Value {
    let changes = engine.snapshot_cache().tracked_items();
    let entries = changes
        .into_iter()
        .map(|(key, trackable)| {
            json!({
                "state": format!("{:?}", trackable.state),
                "key": BASE64_STANDARD.encode(key.to_array()),
                "value": BASE64_STANDARD.encode(&*trackable.item.value_bytes())})
        })
        .collect::<Vec<_>>();
    Value::Array(entries)
}
