use neo_contract::runtime::Value as ContractValue;
use neo_contract::InvocationResult;
use serde_json::json;

pub fn invocation_to_json(result: InvocationResult) -> serde_json::Value {
    let stack = vec![result.value.to_stack_json()];
    let notifications = format_notifications(result.notifications);
    json!({
        "state": "HALT",
        "gasconsumed": result.gas_used,
        "stack": stack,
        "logs": result.logs,
        "notifications": notifications,
    })
}

fn format_notifications(events: Vec<(String, Vec<ContractValue>)>) -> Vec<serde_json::Value> {
    events
        .into_iter()
        .map(|(name, payload)| {
            let state: Vec<serde_json::Value> = payload
                .into_iter()
                .map(|value| value.to_stack_json())
                .collect();
            json!({
                "eventname": name,
                "state": state,
            })
        })
        .collect()
}
