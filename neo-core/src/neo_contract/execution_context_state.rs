// Copyright (C) 2015-2024 The Neo Project.
//
// execution_context_state.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::neo_contract::contract_state::ContractState;
use crate::neo_contract::call_flags::CallFlags;
use crate::uint160::UInt160;
use crate::persistence::DataCache;
use crate::vm::ExecutionContext;

/// Represents the custom state in `ExecutionContext`.
pub struct ExecutionContextState {
    /// The script hash of the current context.
    pub script_hash: UInt160,

    /// The calling context.
    pub calling_context: Option<Box<ExecutionContext>>,

    /// The script hash of the calling native contract. Used in native contracts only.
    pub(crate) native_calling_script_hash: Option<UInt160>,

    /// The `ContractState` of the current context.
    pub contract: Option<ContractState>,

    /// The `CallFlags` of the current context.
    pub call_flags: CallFlags,

    /// The snapshot cache.
    pub snapshot_cache: Option<DataCache>,

    /// The notification count.
    pub notification_count: i32,

    /// Indicates if it's a dynamic call.
    pub is_dynamic_call: bool,
}

impl ExecutionContextState {
    pub fn new() -> Self {
        Self {
            script_hash: UInt160::zero(),
            calling_context: None,
            native_calling_script_hash: None,
            contract: None,
            call_flags: CallFlags::All,
            snapshot_cache: None,
            notification_count: 0,
            is_dynamic_call: false,
        }
    }

    #[deprecated(note = "Use snapshot_cache instead")]
    pub fn snapshot(&self) -> Option<&DataCache> {
        self.snapshot_cache.as_ref()
    }
}
