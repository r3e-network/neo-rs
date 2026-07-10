//! Replay artifact snapshots produced by native block persistence.
//!
//! The protocol persist sequence owns storage mutations; this module only
//! copies engine outputs into replay/indexer records after each engine has
//! completed.

use neo_execution::ApplicationEngine;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::{ApplicationExecuted, Transaction};
use neo_primitives::UInt160;
use neo_vm::StackItem;
use neo_vm_rs::StackValue;
use neo_vm_rs::VmState as VMState;

/// A notification emitted by a native persistence engine, captured for the
/// caller (C# wraps these in `ApplicationExecuted` events).
#[derive(Debug, Clone)]
pub struct NativePersistNotification {
    /// The contract that emitted the notification.
    pub script_hash: UInt160,
    /// The event name (e.g. `Transfer`, `CommitteeChanged`).
    pub event_name: String,
    /// The event arguments.
    pub state: Vec<StackItem>,
}

/// Builds the C# `ApplicationExecuted` record for a finished engine.
///
/// `GasConsumed` is the datoshi fee (C# `engine.FeeConsumed`), the stack is the
/// engine's result stack, and the notifications/logs are the engine's captured
/// events.
pub(crate) fn application_executed<P, B>(
    engine: &ApplicationEngine<P, neo_execution::NoDiagnostic, B>,
    transaction: Option<Transaction>,
    vm_state: VMState,
) -> ApplicationExecuted
where
    P: NativeContractProvider + 'static,
    B: neo_storage::CacheRead,
{
    let mut executed = ApplicationExecuted::new(
        transaction,
        engine.trigger_type(),
        vm_state,
        engine.fault_exception().map(str::to_owned),
        engine.fee_consumed(),
        engine
            .result_stack()
            .iter()
            .map(stack_value_snapshot)
            .collect(),
    );
    executed.notifications = engine.notifications().to_vec();
    executed.logs = engine.logs().to_vec();
    executed
}

pub(crate) fn stack_value_snapshot(item: &StackItem) -> StackValue {
    match item {
        StackItem::Null => StackValue::Null,
        StackItem::Boolean(value) => StackValue::Boolean(*value),
        StackItem::Integer(value) => match value.to_i64() {
            Some(value) => StackValue::Integer(value),
            None => StackValue::BigInteger(value.to_signed_bytes_le()),
        },
        StackItem::ByteString(bytes) => StackValue::ByteString(bytes.clone()),
        StackItem::Buffer(buffer) => StackValue::Buffer(buffer.data()),
        StackItem::Array(array) => StackValue::Array(
            array
                .iter()
                .map(|item| stack_value_snapshot(&item))
                .collect(),
        ),
        StackItem::Struct(structure) => StackValue::Struct(
            structure
                .iter()
                .map(|item| stack_value_snapshot(&item))
                .collect(),
        ),
        StackItem::Map(map) => StackValue::Map(
            map.iter()
                .map(|(key, value)| (stack_value_snapshot(&key), stack_value_snapshot(&value)))
                .collect(),
        ),
        StackItem::Pointer(pointer) => {
            // pointer.position() returns usize. On 64-bit platforms (MSRV 1.85),
            // usize fits in i64. Use an explicit cast with a debug assertion.
            let pos = pointer.position();
            debug_assert!(
                pos <= i64::MAX as usize,
                "pointer position {pos} exceeds i64::MAX"
            );
            // Rationale: Neo VM pointer positions are serialized as signed C#
            // integers; the debug assertion above preserves the cast invariant.
            #[allow(clippy::cast_possible_wrap)]
            StackValue::Pointer(pos as i64)
        }
        StackItem::InteropInterface(_) => StackValue::Interop(0),
    }
}

/// Copies the engine's emitted notifications into the outcome shape.
pub(crate) fn collect_notifications<P, B>(
    engine: &ApplicationEngine<P, neo_execution::NoDiagnostic, B>,
) -> Vec<NativePersistNotification>
where
    P: NativeContractProvider + 'static,
    B: neo_storage::CacheRead,
{
    engine
        .notifications()
        .iter()
        .map(|event| NativePersistNotification {
            script_hash: event.script_hash,
            event_name: event.event_name.clone(),
            state: event.state.clone(),
        })
        .collect()
}
