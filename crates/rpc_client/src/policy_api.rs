// Copyright (C) 2015-2025 The Neo Project.
//
// policy_api.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::{ContractClient, RpcClient};
use neo_core::{NativeContract, UInt160};
use std::sync::Arc;

/// Get Policy info by RPC API
/// Matches C# PolicyAPI
pub struct PolicyApi {
    /// Base contract client functionality
    contract_client: ContractClient,
    /// Policy contract script hash
    script_hash: UInt160,
}

impl PolicyApi {
    /// PolicyAPI Constructor
    /// Matches C# constructor
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self {
            contract_client: ContractClient::new(rpc_client),
            script_hash: NativeContract::policy().hash(),
        }
    }

    /// Get Fee Factor
    /// Matches C# GetExecFeeFactorAsync
    pub async fn get_exec_fee_factor(&self) -> Result<u32, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(&self.script_hash, "getExecFeeFactor", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let value = stack_item.get_integer()?;
        Ok(value.to_u32().ok_or("Invalid fee factor value")?)
    }

    /// Get Storage Price
    /// Matches C# GetStoragePriceAsync
    pub async fn get_storage_price(&self) -> Result<u32, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(&self.script_hash, "getStoragePrice", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let value = stack_item.get_integer()?;
        Ok(value.to_u32().ok_or("Invalid storage price value")?)
    }

    /// Get Network Fee Per Byte
    /// Matches C# GetFeePerByteAsync
    pub async fn get_fee_per_byte(&self) -> Result<i64, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(&self.script_hash, "getFeePerByte", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let value = stack_item.get_integer()?;
        Ok(value.to_i64().ok_or("Invalid fee per byte value")?)
    }

    /// Get Policy Blocked Accounts
    /// Matches C# IsBlockedAsync
    pub async fn is_blocked(&self, account: &UInt160) -> Result<bool, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(&self.script_hash, "isBlocked", vec![account.to_json()])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        Ok(stack_item.get_boolean()?)
    }
}
