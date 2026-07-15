use neo_error::{CoreError, CoreResult};
use neo_vm::rpc_json::StackItemRpcJson;
use neo_vm::{StackItem, VmError};
use serde_json::Value;

pub(super) fn stack_items_rpc_json_per_item(
    items: &[StackItem],
    max_size: usize,
) -> CoreResult<Vec<Value>> {
    items
        .iter()
        .map(|item| {
            StackItemRpcJson::stack_item_rpc_json_deferred_size_check(item, Some(max_size)).map_err(
                |error| match error {
                    VmError::InvalidOperation { operation, reason }
                        if operation == "Max size reached" && reason == operation =>
                    {
                        CoreError::other(operation)
                    }
                    other => other.into(),
                },
            )
        })
        .collect()
}
