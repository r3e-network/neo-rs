use super::super::models::{RpcAccount, RpcTransferOut, RpcUnclaimedGas, RpcValidateAddressResult};
use super::super::{ClientRpcError, Nep17Api, RpcUtility};
use super::RpcClient;
use super::helpers::{
    parse_object_array_result, token_as_boolean, token_as_object, token_as_string,
};
use crate::client::utility::object_array;
use neo_core::BigDecimal;
use neo_json::{JObject, JToken};
use num_bigint::BigInt;
use std::str::FromStr;
use std::sync::Arc;

impl RpcClient {
    /// Close the wallet opened by RPC.
    /// Matches C# `CloseWalletAsync`
    pub async fn close_wallet(&self) -> Result<bool, ClientRpcError> {
        let result = self.rpc_send_async("closewallet", vec![]).await?;
        token_as_boolean(result, "closewallet")
    }

    /// Exports the private key of the specified address.
    /// Matches C# `DumpPrivKeyAsync`
    pub async fn dump_priv_key(&self, address: &str) -> Result<String, ClientRpcError> {
        let result = self
            .rpc_send_async("dumpprivkey", vec![JToken::String(address.to_string())])
            .await?;
        token_as_string(result, "dumpprivkey")
    }

    /// Imports a WIF private key into the wallet opened by RPC.
    /// Matches C# `ImportPrivKeyAsync`
    pub async fn import_priv_key(&self, wif: &str) -> Result<RpcAccount, ClientRpcError> {
        let result = self
            .rpc_send_async("importprivkey", vec![JToken::String(wif.to_string())])
            .await?;
        let obj = token_as_object(result, "importprivkey")?;
        RpcAccount::from_json(&obj).map_err(|err| ClientRpcError::new(-32603, err))
    }

    /// Validates a wallet address.
    /// Matches C# `ValidateAddressAsync`
    pub async fn validate_address(
        &self,
        address: &str,
    ) -> Result<RpcValidateAddressResult, ClientRpcError> {
        let result = self
            .rpc_send_async("validateaddress", vec![JToken::String(address.to_string())])
            .await?;
        let obj = token_as_object(result, "validateaddress")?;
        RpcValidateAddressResult::from_json(&obj).map_err(|err| ClientRpcError::new(-32603, err))
    }

    /// Creates a new account in the wallet opened by RPC.
    /// Matches C# `GetNewAddressAsync`
    pub async fn get_new_address(&self) -> Result<String, ClientRpcError> {
        let result = self.rpc_send_async("getnewaddress", vec![]).await?;
        token_as_string(result, "getnewaddress")
    }

    /// Returns the balance of the specified asset in the wallet.
    /// Matches C# `GetWalletBalanceAsync`
    pub async fn get_wallet_balance(&self, asset_id: &str) -> Result<BigDecimal, ClientRpcError> {
        let result = self
            .rpc_send_async(
                "getwalletbalance",
                vec![JToken::String(asset_id.to_string())],
            )
            .await?;
        let obj = token_as_object(result, "getwalletbalance")?;
        let balance_str = obj
            .get("balance")
            .and_then(neo_json::JToken::as_string)
            .ok_or_else(|| ClientRpcError::new(-32603, "Missing balance in getwalletbalance"))?;
        let balance = BigInt::from_str(&balance_str).map_err(|_| {
            ClientRpcError::new(-32603, format!("Invalid balance value: {balance_str}"))
        })?;
        let asset_hash = RpcUtility::get_script_hash(asset_id, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err))?;
        let nep17 = Nep17Api::new(Arc::new(self.clone()));
        let decimals = nep17
            .decimals(&asset_hash)
            .await
            .map_err(|err| ClientRpcError::new(-32603, err.to_string()))?;
        Ok(BigDecimal::new(balance, decimals))
    }

    /// Gets the amount of unclaimed GAS for an address.
    /// Matches C# `GetUnclaimedGasAsync`
    pub async fn get_unclaimed_gas(
        &self,
        address: &str,
    ) -> Result<RpcUnclaimedGas, ClientRpcError> {
        let result = self
            .rpc_send_async("getunclaimedgas", vec![JToken::String(address.to_string())])
            .await?;
        let obj = token_as_object(result, "getunclaimedgas")?;
        RpcUnclaimedGas::from_json(&obj).map_err(|err| ClientRpcError::new(-32603, err))
    }

    /// Gets the amount of unclaimed GAS in the wallet.
    /// Matches C# `GetWalletUnclaimedGasAsync`
    pub async fn get_wallet_unclaimed_gas(&self) -> Result<BigDecimal, ClientRpcError> {
        let result = self.rpc_send_async("getwalletunclaimedgas", vec![]).await?;
        let value = token_as_string(result, "getwalletunclaimedgas")?;
        let amount = BigInt::from_str(&value).map_err(|_| {
            ClientRpcError::new(-32603, format!("Invalid unclaimed gas value: {value}"))
        })?;
        Ok(BigDecimal::new(amount, 8))
    }

    /// Lists all the accounts in the current wallet.
    /// Matches C# `ListAddressAsync`
    pub async fn list_address(&self) -> Result<Vec<RpcAccount>, ClientRpcError> {
        let result = self.rpc_send_async("listaddress", vec![]).await?;
        parse_object_array_result(
            &result,
            "listaddress returned non-array",
            "listaddress returned null entry",
            "listaddress returned non-object",
            RpcAccount::from_json,
        )
    }

    /// Open wallet file in the provider's machine.
    /// Matches C# `OpenWalletAsync`
    pub async fn open_wallet(&self, path: &str, password: &str) -> Result<bool, ClientRpcError> {
        let result = self
            .rpc_send_async(
                "openwallet",
                vec![
                    JToken::String(path.to_string()),
                    JToken::String(password.to_string()),
                ],
            )
            .await?;
        token_as_boolean(result, "openwallet")
    }

    /// Transfer from the specified address to the destination address.
    /// Matches C# `SendFromAsync`
    pub async fn send_from(
        &self,
        asset_id: &str,
        from_address: &str,
        to_address: &str,
        amount: &str,
    ) -> Result<JObject, ClientRpcError> {
        let params = vec![
            JToken::String(RpcUtility::as_script_hash(asset_id)),
            JToken::String(RpcUtility::as_script_hash(from_address)),
            JToken::String(RpcUtility::as_script_hash(to_address)),
            JToken::String(amount.to_string()),
        ];
        let result = self.rpc_send_async("sendfrom", params).await?;
        token_as_object(result, "sendfrom")
    }

    /// Bulk transfer order, optionally specifying a sender address.
    /// Matches C# `SendManyAsync`
    pub async fn send_many(
        &self,
        from_address: &str,
        outputs: &[RpcTransferOut],
    ) -> Result<JObject, ClientRpcError> {
        let mut params = Vec::new();
        if !from_address.is_empty() {
            params.push(JToken::String(RpcUtility::as_script_hash(from_address)));
        }
        params.push(object_array(outputs, |out| {
            out.to_json(&self.protocol_settings)
        }));
        let result = self.rpc_send_async("sendmany", params).await?;
        token_as_object(result, "sendmany")
    }

    /// Transfer asset from the wallet to the destination address.
    /// Matches C# `SendToAddressAsync`
    pub async fn send_to_address(
        &self,
        asset_id: &str,
        address: &str,
        amount: &str,
    ) -> Result<JObject, ClientRpcError> {
        let params = vec![
            JToken::String(RpcUtility::as_script_hash(asset_id)),
            JToken::String(RpcUtility::as_script_hash(address)),
            JToken::String(amount.to_string()),
        ];
        let result = self.rpc_send_async("sendtoaddress", params).await?;
        token_as_object(result, "sendtoaddress")
    }
}
