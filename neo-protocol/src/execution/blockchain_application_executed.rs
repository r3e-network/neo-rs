// Copyright (c) 2024 R3E Network
// Licensed under the MIT License. See LICENSE file for details.

//! Data representation of an application execution result.
//!
//! This mirrors the C# `Neo.SmartContract.ApplicationExecuted` notification but
//! is deliberately engine-agnostic: it lives in the protocol layer, which has no
//! access to `ApplicationEngine` (that would invert the `neo-smart-contract` →
//! `neo-protocol` dependency). Higher layers construct this value from a
//! finished engine run.

use crate::payloads::Transaction;
use neo_primitives::{LogEventArgs, TriggerType};
use neo_vm::{NotifyEventArgs, StackItem, VmState};

/// Result of executing a script in the application engine, matching the C#
/// `Neo.SmartContract.ApplicationExecuted` notification.
#[derive(Clone)]
pub struct ApplicationExecuted {
    /// The transaction that triggered the execution, if any.
    pub transaction: Option<Transaction>,
    /// The trigger that caused the execution (e.g. application or verification).
    pub trigger: TriggerType,
    /// Final Neo VM state after execution (HALT, FAULT, etc.).
    pub vm_state: VmState,
    /// The fault exception message if execution failed.
    pub exception: Option<String>,
    /// Total GAS consumed by the execution.
    pub gas_consumed: i64,
    /// Result stack items remaining on the evaluation stack.
    pub stack: Vec<StackItem>,
    /// Notifications emitted via `System.Runtime.Notify` during execution.
    pub notifications: Vec<NotifyEventArgs>,
    /// Log messages emitted via `System.Runtime.Log` during execution.
    pub logs: Vec<LogEventArgs>,
}

impl ApplicationExecuted {
    /// Creates an execution result from its constituent parts.
    ///
    /// The application engine adapter (a higher layer) populates these fields
    /// after driving a script to completion; the protocol layer only carries the
    /// resulting data.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transaction: Option<Transaction>,
        trigger: TriggerType,
        vm_state: VmState,
        exception: Option<String>,
        gas_consumed: i64,
        stack: Vec<StackItem>,
        notifications: Vec<NotifyEventArgs>,
        logs: Vec<LogEventArgs>,
    ) -> Self {
        Self {
            transaction,
            trigger,
            vm_state,
            exception,
            gas_consumed,
            stack,
            notifications,
            logs,
        }
    }
}
