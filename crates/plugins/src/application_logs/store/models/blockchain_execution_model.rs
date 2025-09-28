// Copyright (C) 2015-2025 The Neo Project.
//
// blockchain_execution_model.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::application_logs::store::states::ExecutionLogState;
use crate::application_logs::log_reader::TriggerType;
use crate::application_logs::store::models::{BlockchainEventModel, ApplicationEngineLogModel};
use neo_core::UInt160;
use neo_vm::{VMState, StackItem};

/// Blockchain execution model matching C# BlockchainExecutionModel exactly
#[derive(Debug, Clone)]
pub struct BlockchainExecutionModel {
    /// Trigger type
    pub trigger: TriggerType,
    /// VM state
    pub vm_state: VMState,
    /// Exception message
    pub exception: String,
    /// Gas consumed
    pub gas_consumed: i64,
    /// Stack items
    pub stack: Vec<Box<dyn StackItem>>,
    /// Notifications
    pub notifications: Vec<BlockchainEventModel>,
    /// Logs
    pub logs: Vec<ApplicationEngineLogModel>,
}

impl BlockchainExecutionModel {
    /// Creates a new BlockchainExecutionModel
    /// Matches C# Create method
    pub fn create(
        trigger: TriggerType,
        execution_log_state: ExecutionLogState,
        stack: Vec<Box<dyn StackItem>>,
    ) -> Self {
        Self {
            trigger,
            vm_state: execution_log_state.vm_state,
            exception: execution_log_state.exception.unwrap_or_default(),
            gas_consumed: execution_log_state.gas_consumed,
            stack,
            notifications: Vec::new(),
            logs: Vec::new(),
        }
    }
}