use neo_payloads::{ApplicationExecuted, NotifyEventArgs};
use neo_primitives::{TriggerType, UInt256};
use neo_vm::VmState as VMState;
use neo_vm::rpc_json::StackItemRpcJson;
use serde_json::{Map, Value};

use super::stack_json::stack_items_rpc_json_per_item;

pub(super) fn block_log_json(
    block_hash: &UInt256,
    executions: &[ApplicationExecuted],
    debug: bool,
    max_stack_size: usize,
) -> Value {
    let block_executions = executions
        .iter()
        .filter(|exec| exec.transaction.is_none())
        .map(|exec| execution_to_json(exec, false, debug, max_stack_size))
        .collect::<Vec<_>>();
    let mut obj = Map::new();
    obj.insert(
        "blockhash".to_string(),
        Value::String(block_hash.to_string()),
    );
    obj.insert("executions".to_string(), Value::Array(block_executions));
    Value::Object(obj)
}

pub(super) fn transaction_log_json(
    tx_hash: &UInt256,
    exec: &ApplicationExecuted,
    debug: bool,
    max_stack_size: usize,
) -> Value {
    let mut obj = Map::new();
    obj.insert("txid".to_string(), Value::String(tx_hash.to_string()));
    obj.insert(
        "executions".to_string(),
        Value::Array(vec![execution_to_json(exec, true, debug, max_stack_size)]),
    );
    Value::Object(obj)
}

fn execution_to_json(
    exec: &ApplicationExecuted,
    include_exception: bool,
    debug: bool,
    max_stack_size: usize,
) -> Value {
    let mut trigger = Map::new();
    trigger.insert(
        "trigger".to_string(),
        Value::String(trigger_to_string(exec.trigger).to_string()),
    );
    trigger.insert(
        "vmstate".to_string(),
        Value::String(vm_state_to_string(exec.vm_state).to_string()),
    );
    trigger.insert(
        "gasconsumed".to_string(),
        Value::String(exec.gas_consumed.to_string()),
    );

    let mut exception = include_exception.then(|| exec.exception.clone()).flatten();
    match stack_items_rpc_json_per_item(&exec.stack, max_stack_size) {
        Ok(stack) => {
            trigger.insert("stack".to_string(), Value::Array(stack));
        }
        Err(err) => {
            exception = Some(err.to_string());
        }
    }

    if include_exception || exception.is_some() {
        trigger.insert(
            "exception".to_string(),
            exception.map(Value::String).unwrap_or(Value::Null),
        );
    }

    let notifications = exec
        .notifications
        .iter()
        .map(notification_to_json)
        .collect::<Vec<_>>();
    trigger.insert("notifications".to_string(), Value::Array(notifications));

    if debug {
        let logs = exec
            .logs
            .iter()
            .map(|log| {
                let mut obj = Map::new();
                obj.insert(
                    "contract".to_string(),
                    Value::String(log.script_hash.to_string()),
                );
                obj.insert("message".to_string(), Value::String(log.message.clone()));
                Value::Object(obj)
            })
            .collect();
        trigger.insert("logs".to_string(), Value::Array(logs));
    }

    Value::Object(trigger)
}

fn trigger_to_string(trigger: TriggerType) -> &'static str {
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

fn vm_state_to_string(state: VMState) -> &'static str {
    match state {
        VMState::NONE => "NONE",
        VMState::HALT => "HALT",
        VMState::FAULT => "FAULT",
        VMState::BREAK => "BREAK",
    }
}

fn notification_to_json(event: &NotifyEventArgs) -> Value {
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
        .state()
        .iter()
        .map(|item| StackItemRpcJson::stack_item_rpc_json(item, None))
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
