use crate::{ContractClient, RpcClient, RpcClientError, RpcUtility};
use neo_native_contracts::PolicyContract;
use neo_primitives::UInt160;
use num_traits::cast::ToPrimitive;
use std::sync::Arc;

/// Get Policy info by RPC API
/// Matches C# `PolicyAPI`
pub struct PolicyApi {
    /// Base contract client functionality
    contract_client: ContractClient,
    /// Policy contract script hash
    script_hash: UInt160,
}

impl PolicyApi {
    /// `PolicyAPI` Constructor
    /// Matches C# constructor
    #[must_use]
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self {
            contract_client: ContractClient::new(rpc_client),
            script_hash: PolicyContract::new().hash(),
        }
    }

    /// Get Fee Factor
    /// Matches C# `GetExecFeeFactorAsync`
    pub async fn get_exec_fee_factor(&self) -> Result<u32, RpcClientError> {
        let result = self
            .contract_client
            .test_invoke(&self.script_hash, "getExecFeeFactor", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let value = RpcUtility::stack_value_to_bigint(stack_item)?;
        Ok(value.to_u32().ok_or("Invalid fee factor value")?)
    }

    /// Get Storage Price
    /// Matches C# `GetStoragePriceAsync`
    pub async fn get_storage_price(&self) -> Result<u32, RpcClientError> {
        let result = self
            .contract_client
            .test_invoke(&self.script_hash, "getStoragePrice", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let value = RpcUtility::stack_value_to_bigint(stack_item)?;
        Ok(value.to_u32().ok_or("Invalid storage price value")?)
    }

    /// Get Network Fee Per Byte
    /// Matches C# `GetFeePerByteAsync`
    pub async fn get_fee_per_byte(&self) -> Result<i64, RpcClientError> {
        let result = self
            .contract_client
            .test_invoke(&self.script_hash, "getFeePerByte", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let value = RpcUtility::stack_value_to_bigint(stack_item)?;
        Ok(value.to_i64().ok_or("Invalid fee per byte value")?)
    }

    /// Get Policy Blocked Accounts
    /// Matches C# `IsBlockedAsync`
    pub async fn is_blocked(&self, account: &UInt160) -> Result<bool, RpcClientError> {
        let result = self
            .contract_client
            .test_invoke(
                &self.script_hash,
                "isBlocked",
                vec![serde_json::json!(account.to_string())],
            )
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        Ok(RpcUtility::stack_value_to_bool(stack_item))
    }
}

#[cfg(test)]
#[path = "../../tests/client/policy_api.rs"]
mod tests;
