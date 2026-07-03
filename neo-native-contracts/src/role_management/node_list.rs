//! C# `RoleManagement.NodeList` stack-value and storage codecs.

use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_serialization::BinarySerializer;
use neo_vm::StackItem;
use neo_vm_rs::{ExecutionEngineLimits, StackValue};

/// Decodes a serialized node-list (a `BinarySerializer` array of compressed
/// EC-point byte strings) into `ECPoint`s.
pub(super) fn decode_node_list(value: &[u8]) -> CoreResult<Vec<ECPoint>> {
    let limits = ExecutionEngineLimits::default();
    let value = BinarySerializer::deserialize_stack_value_with_limits(
        value,
        limits.max_item_size as usize,
        limits.max_stack_size as usize,
    )
    .map_err(|e| CoreError::deserialization(format!("RoleManagement node list: {e}")))?;
    Ok(NodeList::from_stack_value(value)?.into_nodes())
}

/// Serializes an empty node list (C# returns an empty `ECPoint[]`, not `null`,
/// when no designation exists).
pub(super) fn empty_node_list() -> CoreResult<Vec<u8>> {
    let item = NodeList::new(Vec::new()).to_stack_value();
    BinarySerializer::serialize_stack_value_default(&item)
        .map_err(|e| CoreError::invalid_operation(format!("RoleManagement empty list: {e}")))
}

/// Builds the persisted `StackValue::Array` representation for C# `NodeList`.
pub(super) fn nodes_to_stack_value(points: &[ECPoint]) -> StackValue {
    NodeList::new(points.to_vec()).to_stack_value()
}

/// Adapts the canonical node-list `StackValue` projection to the live VM
/// notification boundary, preserving the caller-provided order.
pub(super) fn nodes_to_event_array(points: &[ECPoint]) -> CoreResult<StackItem> {
    StackItem::try_from(nodes_to_stack_value(points)).map_err(|error| {
        CoreError::invalid_operation(format!("RoleManagement event node list: {error}"))
    })
}

/// Serializes a node list as C# `NodeList` stores it: a `BinarySerializer` array
/// of compressed EC-point byte strings, with the points sorted ascending
/// (`list.Sort()`). The stored order differs from the event's `newNodes` order
/// (which preserves the caller's input order).
pub(super) fn encode_node_list(points: &[ECPoint]) -> CoreResult<Vec<u8>> {
    let mut sorted = points.to_vec();
    sorted.sort();
    BinarySerializer::serialize_stack_value_default(&nodes_to_stack_value(&sorted))
        .map_err(|e| CoreError::invalid_operation(format!("RoleManagement node list: {e}")))
}

/// Decodes + validates the `nodes` Array argument: 1..=32 compressed EC points
/// (C# `nodes.Length == 0 || nodes.Length > 32` guard).
pub(super) fn parse_nodes_arg(bytes: &[u8]) -> CoreResult<Vec<ECPoint>> {
    let points = decode_node_list(bytes)?;
    if points.is_empty() || points.len() > 32 {
        return Err(CoreError::invalid_operation(format!(
            "RoleManagement: nodes count {} must be between 1 and 32",
            points.len()
        )));
    }
    Ok(points)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NodeList {
    nodes: Vec<ECPoint>,
}

impl NodeList {
    pub(super) fn new(nodes: Vec<ECPoint>) -> Self {
        Self { nodes }
    }

    pub(super) fn into_nodes(self) -> Vec<ECPoint> {
        self.nodes
    }

    pub(super) fn to_stack_value(&self) -> StackValue {
        StackValue::Array(
            neo_vm_rs::next_stack_item_id(),
            self.nodes
                .iter()
                .map(|point| StackValue::ByteString(point.to_bytes()))
                .collect(),
        )
    }

    pub(super) fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let StackValue::Array(_, items) = stack_value else {
            return Err(CoreError::invalid_data(
                "RoleManagement node list is not an array",
            ));
        };
        let mut nodes = Vec::with_capacity(items.len());
        for entry in items {
            let bytes = entry.to_byte_string_bytes().ok_or_else(|| {
                CoreError::invalid_data("RoleManagement node entry is not byte-like")
            })?;
            nodes.push(ECPoint::from_bytes(&bytes).map_err(|e| {
                CoreError::invalid_data(format!("RoleManagement node EC point: {e}"))
            })?);
        }
        Ok(Self { nodes })
    }
}

neo_vm::impl_interoperable_via_stack_value!(NodeList);
