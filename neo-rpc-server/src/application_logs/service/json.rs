use super::*;
pub(super) fn trigger_to_string(trigger: TriggerType) -> &'static str {
    if trigger == TriggerType::ON_PERSIST {
        "OnPersist"
    } else if trigger == TriggerType::POST_PERSIST {
        "PostPersist"
    } else if trigger == TriggerType::VERIFICATION {
        "Verification"
    } else if trigger == TriggerType::APPLICATION {
        "Application"
    } else if trigger == TriggerType::SYSTEM {
        "System"
    } else if trigger == TriggerType::ALL {
        "All"
    } else {
        "Unknown"
    }
}

/// Application-log exception text aligned with C# `ExecutionLogState`.
///
/// C# stores `Exception.InnerException.Message`, or `Exception.Message` when
/// there is no inner exception. `ApplicationEngine.AddFee` throws
/// `InvalidOperationException("Insufficient GAS.")`. Rust records the same
/// condition with its own wording, and `ExecutionEngine::on_fault` appends
/// ` [ip=N opcode=NAME eval_depth=N]`. That suffix is diagnostic only.
pub(super) fn vm_state_to_string(state: VMState) -> &'static str {
    match state {
        VMState::NONE => "NONE",
        VMState::HALT => "HALT",
        VMState::FAULT => "FAULT",
        VMState::BREAK => "BREAK",
    }
}

pub(super) fn notification_to_json(event: &NotifyEventArgs) -> Value {
    let mut notification = Map::new();
    notification.insert(
        "contract".to_string(),
        Value::String(event.script_hash.to_string()),
    );
    notification.insert(
        "eventname".to_string(),
        Value::String(event.event_name.clone()),
    );

    let state_values = event
        .state
        .iter()
        .map(|item| stack_item_rpc_json(item, None))
        .collect::<Result<Vec<_>, _>>();

    let state = match state_values {
        Ok(values) => {
            let mut state_obj = Map::new();
            state_obj.insert("type".to_string(), Value::String("Array".to_string()));
            state_obj.insert("value".to_string(), Value::Array(values));
            Value::Object(state_obj)
        }
        Err(_) => Value::String("error: recursive reference".to_string()),
    };
    notification.insert("state".to_string(), state);

    Value::Object(notification)
}
