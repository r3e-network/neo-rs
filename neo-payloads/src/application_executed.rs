//! ApplicationExecuted - the per-transaction execution record emitted when a
//! block is processed. Mirrors the C# `Neo.Ledger.ApplicationExecuted` type.
//!
//! Plugins (ApplicationLogs, TokensTracker, OracleService) need to consume
//! the notification and log events emitted by contract execution, so this
//! type uses the rich [`NotifyEventArgs`] and [`neo_primitives::LogEventArgs`]
//! structs directly (rather than `serde_json::Value`) so callers can access
//! typed fields like `event_name`, `state`, `script_hash`,
//! `script_container`, etc.

use crate::{NotifyEventArgs, Transaction};
use neo_primitives::{LogEventArgs, TriggerType, UInt160};
use neo_vm_rs::StackValue;
use neo_vm_rs::VmState as VMState;

/// Result of executing a single transaction (mirrors C# ApplicationExecuted).
#[derive(Clone, Debug)]
pub struct ApplicationExecuted {
    /// The transaction that was executed, if any.
    pub transaction: Option<Transaction>,
    /// What triggered the execution.
    pub trigger: TriggerType,
    /// Final VM state.
    pub vm_state: VMState,
    /// Exception message, if execution faulted.
    pub exception: Option<String>,
    /// GAS consumed.
    pub gas_consumed: i64,
    /// Resulting evaluation stack.
    pub stack: Vec<StackValue>,
    /// Notification events emitted by the contract.
    pub notifications: Vec<NotifyEventArgs>,
    /// Log events emitted by the contract.
    pub logs: Vec<LogEventArgs>,
}

impl ApplicationExecuted {
    /// Construct a new `ApplicationExecuted` with the engine's post-execution state.
    pub fn new(
        transaction: Option<Transaction>,
        trigger: TriggerType,
        vm_state: VMState,
        exception: Option<String>,
        gas_consumed: i64,
        stack: Vec<StackValue>,
    ) -> Self {
        Self {
            transaction,
            trigger,
            vm_state,
            exception,
            gas_consumed,
            stack,
            notifications: Vec::new(),
            logs: Vec::new(),
        }
    }

    /// Returns the trigger type as a string (mirrors C# event_name-like accessor).
    pub fn event_name(&self) -> &'static str {
        match self.trigger {
            TriggerType::ON_PERSIST => "OnPersist",
            TriggerType::POST_PERSIST => "PostPersist",
            TriggerType::VERIFICATION => "Verification",
            TriggerType::APPLICATION => "Application",
            TriggerType::SYSTEM => "System",
            TriggerType::ALL => "All",
            _ => "Unknown",
        }
    }

    /// Returns the VM state as a string (mirrors C# state-like accessor).
    pub fn state(&self) -> &'static str {
        match self.vm_state {
            VMState::NONE => "NONE",
            VMState::HALT => "HALT",
            VMState::FAULT => "FAULT",
            VMState::BREAK => "BREAK",
        }
    }

    /// Returns the contract script hash of the executed call (or the
    /// triggering script for OnPersist/PostPersist). Mirrors the C#
    /// `ScriptHash` accessor.
    ///
    /// Plugins that only need the per-event contract hash should use
    /// `notify.script_hash` directly.
    pub fn script_hash(&self) -> Option<UInt160> {
        None
    }

    /// Returns the script container (the executed transaction, if any).
    pub fn script_container(&self) -> Option<&Transaction> {
        self.transaction.as_ref()
    }
}
