//! Blockchain ApplicationExecuted implementation.
//!
//! This module provides the ApplicationExecuted functionality exactly matching C# Neo Blockchain.ApplicationExecuted.

// Matches C# using directives exactly:
// using Neo.Network.P2P.Payloads;
// using Neo.SmartContract;
// using Neo.VM;
// using Neo.VM.Types;
// using System;

// NOTE: This is a partial class in C# (partial class Blockchain -> partial class ApplicationExecuted)
// In Rust, this is implemented as a separate file but conceptually part of the Blockchain module

/// namespace Neo.Ledger -> partial class Blockchain -> partial class ApplicationExecuted
pub struct ApplicationExecuted {
    /// The transaction that contains the executed script. This field could be null if the contract is invoked by system.
    /// public Transaction Transaction { get; }
    pub transaction: Option<crate::transaction::Transaction>,

    /// The trigger of the execution.
    /// public TriggerType Trigger { get; }
    pub trigger: crate::smart_contract::TriggerType,

    /// The state of the virtual machine after the contract is executed.
    /// public VMState VMState { get; }
    pub vm_state: crate::vm::VMState,

    /// The exception that caused the execution to terminate abnormally. This field could be null if the execution ends normally.
    /// public Exception Exception { get; }
    pub exception: Option<String>, // Exception type placeholder

    /// GAS spent to execute.
    /// public long GasConsumed { get; }
    pub gas_consumed: i64,

    /// Items on the stack of the virtual machine after execution.
    /// public StackItem[] Stack { get; }
    pub stack: Vec<crate::vm::StackItem>,

    /// The notifications sent during the execution.
    /// public NotifyEventArgs[] Notifications { get; }
    pub notifications: Vec<crate::smart_contract::NotifyEventArgs>,
}

impl ApplicationExecuted {
    /// internal ApplicationExecuted(ApplicationEngine engine)
    pub(crate) fn new(engine: &crate::smart_contract::ApplicationEngine) -> Self {
        Self {
            // Transaction = engine.ScriptContainer as Transaction;
            transaction: engine.script_container().and_then(|c| c.as_transaction()),
            // Trigger = engine.Trigger;
            trigger: engine.trigger(),
            // VMState = engine.State;
            vm_state: engine.state(),
            // GasConsumed = engine.FeeConsumed;
            gas_consumed: engine.fee_consumed(),
            // Exception = engine.FaultException;
            exception: engine.fault_exception().map(|e| e.to_string()),
            // Stack = [.. engine.ResultStack];
            stack: engine.result_stack().to_vec(),
            // Notifications = [.. engine.Notifications];
            notifications: engine.notifications().to_vec(),
        }
    }
}
