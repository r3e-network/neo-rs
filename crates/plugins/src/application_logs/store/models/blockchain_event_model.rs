// Copyright (C) 2015-2025 The Neo Project.
//
// blockchain_event_model.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::application_logs::store::states::{NotifyLogState, ContractLogState};
use neo_core::UInt160;
use neo_vm::StackItem;

/// Blockchain event model matching C# BlockchainEventModel exactly
#[derive(Debug, Clone)]
pub struct BlockchainEventModel {
    /// Script hash
    pub script_hash: UInt160,
    /// Event name
    pub event_name: String,
    /// State items
    pub state: Vec<Box<dyn StackItem>>,
}

impl BlockchainEventModel {
    /// Creates a new BlockchainEventModel with parameters
    /// Matches C# Create method with parameters
    pub fn create_with_params(
        script_hash: UInt160,
        event_name: String,
        state: Vec<Box<dyn StackItem>>,
    ) -> Self {
        Self {
            script_hash,
            event_name: event_name.unwrap_or_default(),
            state,
        }
    }
    
    /// Creates a new BlockchainEventModel from NotifyLogState
    /// Matches C# Create method with NotifyLogState
    pub fn create_from_notify_log_state(
        notify_log_state: NotifyLogState,
        state: Vec<Box<dyn StackItem>>,
    ) -> Self {
        Self {
            script_hash: notify_log_state.script_hash,
            event_name: notify_log_state.event_name,
            state,
        }
    }
    
    /// Creates a new BlockchainEventModel from ContractLogState
    /// Matches C# Create method with ContractLogState
    pub fn create_from_contract_log_state(
        contract_log_state: ContractLogState,
        state: Vec<Box<dyn StackItem>>,
    ) -> Self {
        Self {
            script_hash: contract_log_state.script_hash,
            event_name: contract_log_state.event_name,
            state,
        }
    }
}