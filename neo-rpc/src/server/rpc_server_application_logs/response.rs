//! Response shaping helpers for ApplicationLogs RPC handlers.
//!
//! The service stores already-rendered C#-compatible JSON. This module only
//! applies the optional trigger filter required by `getapplicationlog`.

use neo_primitives::TriggerType;
use serde_json::Value;
use std::str::FromStr;

pub(super) fn apply_trigger_filter(mut raw: Value, trigger_filter: Option<&str>) -> Value {
    let Some(filter) = trigger_filter else {
        return raw;
    };
    if TriggerType::from_str(filter).is_err() {
        return raw;
    }
    if let Value::Object(obj) = &mut raw {
        if let Some(Value::Array(executions)) = obj.get_mut("executions") {
            executions.retain(|execution| {
                execution
                    .get("trigger")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value.eq_ignore_ascii_case(filter))
            });
        }
    }
    raw
}
