use super::super::ClientRpcError;
use super::super::models::{
    RpcNep11Balances, RpcNep11Transfers, RpcNep17Balances, RpcNep17Transfers,
};
use super::RpcClient;
use super::helpers::token_as_object;
use super::hooks::RpcObserver;
use crate::types::RpcContractState;
use neo_serialization::json::{JObject, JToken};

impl<O> RpcClient<O>
where
    O: RpcObserver,
{
    /// Gets NEP-17 transfers.
    pub async fn get_nep17_transfers(
        &self,
        address: &str,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> Result<RpcNep17Transfers, ClientRpcError> {
        let mut params = vec![JToken::String(address.to_string())];
        if let Some(start) = start_time {
            params.push(JToken::Number(start as f64));
        }
        if let Some(end) = end_time {
            params.push(JToken::Number(end as f64));
        }

        let result = self.rpc_send_async("getnep17transfers", params).await?;
        let obj = token_as_object(result, "getnep17transfers")?;
        RpcNep17Transfers::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Gets NEP-17 balances.
    pub async fn get_nep17_balances(
        &self,
        address: &str,
    ) -> Result<RpcNep17Balances, ClientRpcError> {
        let result = self
            .rpc_send_async(
                "getnep17balances",
                vec![JToken::String(address.to_string())],
            )
            .await?;
        let obj = token_as_object(result, "getnep17balances")?;
        RpcNep17Balances::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Gets NEP-11 transfers.
    pub async fn get_nep11_transfers(
        &self,
        address: &str,
    ) -> Result<RpcNep11Transfers, ClientRpcError> {
        let result = self
            .rpc_send_async(
                "getnep11transfers",
                vec![JToken::String(address.to_string())],
            )
            .await?;
        let obj = token_as_object(result, "getnep11transfers")?;
        RpcNep11Transfers::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Gets NEP-11 balances.
    pub async fn get_nep11_balances(
        &self,
        address: &str,
    ) -> Result<RpcNep11Balances, ClientRpcError> {
        let result = self
            .rpc_send_async(
                "getnep11balances",
                vec![JToken::String(address.to_string())],
            )
            .await?;
        let obj = token_as_object(result, "getnep11balances")?;
        RpcNep11Balances::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Gets contract state by hash.
    pub async fn get_contract_state(&self, hash: &str) -> Result<RpcContractState, ClientRpcError> {
        let result = self
            .rpc_send_async("getcontractstate", vec![JToken::String(hash.to_string())])
            .await?;
        let obj = token_as_object(result, "getcontractstate")?;
        RpcContractState::from_json(&obj)
            .map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Gets contract state by numeric contract ID.
    pub async fn get_contract_state_by_id(
        &self,
        id: i32,
    ) -> Result<RpcContractState, ClientRpcError> {
        let result = self
            .rpc_send_async("getcontractstate", vec![JToken::Number(f64::from(id))])
            .await?;
        let obj = token_as_object(result, "getcontractstate")?;
        RpcContractState::from_json(&obj)
            .map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Gets NEP-11 properties.
    pub async fn get_nep11_properties(
        &self,
        nep11_contract: &str,
        token_id_hex: &str,
    ) -> Result<JObject, ClientRpcError> {
        let params = vec![
            JToken::String(nep11_contract.to_string()),
            JToken::String(token_id_hex.to_string()),
        ];

        let result = self.rpc_send_async("getnep11properties", params).await?;
        token_as_object(result, "getnep11properties")
    }
}
