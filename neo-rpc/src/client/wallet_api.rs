// Copyright (C) 2015-2025 The Neo Project.
//
// wallet_api.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::models::RpcTransaction;
use crate::{Nep17Api, RpcClient, RpcError, RpcUtility};
use neo_primitives::BigDecimal;
use neo_native_contracts::{GasToken, NeoToken};
use neo_wallets::wallet_helper as WalletHelper;
use neo_payloads::{Transaction};
use neo_crypto::ECPoint;
use neo_wallets::KeyPair;
use neo_execution::{Contract, NativeContract};
use neo_primitives::UInt160;
use num_bigint::BigInt;
use num_traits::cast::ToPrimitive;
use std::sync::Arc;

/// Wallet Common APIs
/// Matches C# `WalletAPI`
pub struct WalletApi {
    /// The RPC client instance
    rpc_client: Arc<RpcClient>,
    /// NEP17 API for token operations
    nep17_api: Nep17Api}

impl WalletApi {
    /// `WalletAPI` Constructor
    /// Matches C# constructor
    #[must_use]
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self {
            nep17_api: Nep17Api::new(rpc_client.clone()),
            rpc_client}
   }

    /// Get unclaimed gas with address, scripthash or public key string
    /// Matches C# `GetUnclaimedGasAsync` with string parameter
    pub async fn get_unclaimed_gas(&self, account: &str) -> Result<f64, RpcError> {
        let account_hash =
            RpcUtility::get_script_hash(account, &self.rpc_client.protocol_settings)?;
        self.get_unclaimed_gas_from_hash(&account_hash).await
   }

    /// Get unclaimed gas
    /// Matches C# `GetUnclaimedGasAsync` with `UInt160` parameter
    pub async fn get_unclaimed_gas_from_hash(&self, account: &UInt160) -> Result<f64, RpcError> {
        let script_hash = neo_hash();
        let block_count = self.rpc_client.get_block_count().await?;

        let result = self
            .nep17_api
            .contract_client()
            .test_invoke(
                &script_hash,
                "unclaimedGas",
                vec![
                    serde_json::json!(account.to_string()),
                    serde_json::json!(block_count - 1),
                ],
            )
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let balance = RpcUtility::stack_value_to_bigint(stack_item)?;
        let gas_factor = gas_factor();

        Ok(balance.to_f64().unwrap_or(0.0) / gas_factor as f64)
   }

    /// Get Neo Balance
    /// Matches C# `GetNeoBalanceAsync`
    pub async fn get_neo_balance(&self, account: &str) -> Result<u32, RpcError> {
        let balance = self
            .get_token_balance(&neo_hash().to_string(), account)
            .await?;
        Ok(balance.to_u32().ok_or("Invalid NEO balance")?)
   }

    /// Get Gas Balance
    /// Matches C# `GetGasBalanceAsync`
    pub async fn get_gas_balance(&self, account: &str) -> Result<f64, RpcError> {
        let balance = self
            .get_token_balance(&gas_hash().to_string(), account)
            .await?;
        let gas_factor = gas_factor();
        Ok(balance.to_f64().unwrap_or(0.0) / gas_factor as f64)
   }

    /// Get token balance with string parameters
    /// Matches C# `GetTokenBalanceAsync`
    pub async fn get_token_balance(
        &self,
        token_hash: &str,
        account: &str,
    ) -> Result<BigInt, RpcError> {
        let token_script_hash =
            RpcUtility::get_script_hash(token_hash, &self.rpc_client.protocol_settings)?;
        let account_hash =
            RpcUtility::get_script_hash(account, &self.rpc_client.protocol_settings)?;

        self.nep17_api
            .balance_of(&token_script_hash, &account_hash)
            .await
   }

    /// Claim GAS from NEO
    /// Matches C# `ClaimGasAsync`
    pub async fn claim_gas(&self, key: &KeyPair) -> Result<Transaction, RpcError> {
        self.claim_gas_with_assert(key, true).await
   }

    /// Claim GAS using WIF or private key string.
    /// Matches C# ClaimGasAsync(string)
    pub async fn claim_gas_from_key(&self, key: &str) -> Result<Transaction, RpcError> {
        self.claim_gas_from_key_with_assert(key, true).await
   }

    /// Claim GAS using WIF or private key string with optional assert emission.
    pub async fn claim_gas_from_key_with_assert(
        &self,
        key: &str,
        add_assert: bool,
    ) -> Result<Transaction, RpcError> {
        let key_pair = RpcUtility::key_pair(key).map_err(|e| RpcError::Other(e.to_string()))?;
        self.claim_gas_with_assert(&key_pair, add_assert).await
   }

    /// Claim GAS with optional assert emission.
    pub async fn claim_gas_with_assert(
        &self,
        key: &KeyPair,
        add_assert: bool,
    ) -> Result<Transaction, RpcError> {
        let sender_script = Contract::create_signature_redeem_script(key.get_public_key_point()?);
        let sender = UInt160::from_script(&sender_script);

        self.claim_gas_from_account_with_assert(&sender, key, add_assert)
            .await
   }

    /// Claim GAS from specific account
    /// Matches C# `ClaimGasAsync` with account parameter
    pub async fn claim_gas_from_account(
        &self,
        account: &UInt160,
        key: &KeyPair,
    ) -> Result<Transaction, RpcError> {
        self.claim_gas_from_account_with_assert(account, key, true)
            .await
   }

    /// Claim GAS from specific account with optional assert emission.
    pub async fn claim_gas_from_account_with_assert(
        &self,
        account: &UInt160,
        key: &KeyPair,
        add_assert: bool,
    ) -> Result<Transaction, RpcError> {
        let neo_balance = self.nep17_api.balance_of(&neo_hash(), account).await?;

        if neo_balance == BigInt::from(0) {
            return Err("No NEO balance to claim GAS from".into());
       }

        // Transfer NEO to self to trigger GAS claim
        let tx = self
            .nep17_api
            .create_transfer_tx_with_from_and_assert(
                &neo_hash(),
                account,
                key,
                account,
                neo_balance,
                None,
                add_assert,
            )
            .await?;

        let _hash = self.rpc_client.send_raw_transaction(&tx).await?;
        Ok(tx)
   }

    /// Transfer NEP17 token
    /// Matches C# `TransferAsync`
    pub async fn transfer(
        &self,
        token_hash: &str,
        key: &KeyPair,
        to_address: &str,
        amount: BigInt,
        data: Option<serde_json::Value>,
    ) -> Result<(Transaction, String), RpcError> {
        self.transfer_with_assert(token_hash, key, to_address, amount, data, true)
            .await
   }

    /// Transfer NEP17 token with optional assert emission.
    pub async fn transfer_with_assert(
        &self,
        token_hash: &str,
        key: &KeyPair,
        to_address: &str,
        amount: BigInt,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<(Transaction, String), RpcError> {
        let token_script_hash =
            RpcUtility::get_script_hash(token_hash, &self.rpc_client.protocol_settings)?;
        let to = RpcUtility::get_script_hash(to_address, &self.rpc_client.protocol_settings)?;

        let tx = self
            .nep17_api
            .create_transfer_tx_with_assert(&token_script_hash, key, &to, amount, data, add_assert)
            .await?;

        let hash = self.rpc_client.send_raw_transaction(&tx).await?;
        Ok((tx, hash.to_string()))
   }

    /// Transfer NEP17 token using decimal amount and WIF/private key string.
    /// Matches C# TransferAsync(string tokenHash, string fromKey, string toAddress, decimal amount).
    pub async fn transfer_decimal_from_key(
        &self,
        token_hash: &str,
        from_key: &str,
        to_address: &str,
        amount: BigDecimal,
        data: Option<serde_json::Value>,
    ) -> Result<(Transaction, String), RpcError> {
        self.transfer_decimal_from_key_with_assert(
            token_hash, from_key, to_address, amount, data, true,
        )
        .await
   }

    /// Transfer NEP17 token using decimal amount and WIF/private key string with optional assert.
    pub async fn transfer_decimal_from_key_with_assert(
        &self,
        token_hash: &str,
        from_key: &str,
        to_address: &str,
        amount: BigDecimal,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<(Transaction, String), RpcError> {
        let key_pair =
            RpcUtility::key_pair(from_key).map_err(|e| RpcError::Other(e.to_string()))?;
        self.transfer_decimal_with_assert(
            token_hash, &key_pair, to_address, amount, data, add_assert,
        )
        .await
   }

    /// Transfer NEP17 token using decimal amount.
    /// Matches C# TransferAsync(string tokenHash, string fromKey, string toAddress, decimal amount)
    /// after key parsing.
    pub async fn transfer_decimal(
        &self,
        token_hash: &str,
        key: &KeyPair,
        to_address: &str,
        amount: BigDecimal,
        data: Option<serde_json::Value>,
    ) -> Result<(Transaction, String), RpcError> {
        self.transfer_decimal_with_assert(token_hash, key, to_address, amount, data, true)
            .await
   }

    /// Transfer NEP17 token using decimal amount with optional assert emission.
    pub async fn transfer_decimal_with_assert(
        &self,
        token_hash: &str,
        key: &KeyPair,
        to_address: &str,
        amount: BigDecimal,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<(Transaction, String), RpcError> {
        let token_script_hash =
            RpcUtility::get_script_hash(token_hash, &self.rpc_client.protocol_settings)?;
        let decimals = self.nep17_api.decimals(&token_script_hash).await?;
        let amount_integer = amount
            .to_big_integer(decimals)
            .map_err(|e| RpcError::Other(e.to_string()))?;

        let to = RpcUtility::get_script_hash(to_address, &self.rpc_client.protocol_settings)?;
        let tx = self
            .nep17_api
            .create_transfer_tx_with_assert(
                &token_script_hash,
                key,
                &to,
                amount_integer,
                data,
                add_assert,
            )
            .await?;

        let hash = self.rpc_client.send_raw_transaction(&tx).await?;
        Ok((tx, hash.to_string()))
   }

    #[allow(clippy::too_many_arguments)]
    /// Transfer NEP17 token from multi-sig account.
    /// Matches C# `TransferAsync` multi-sig overload.
    pub async fn transfer_multi_sig(
        &self,
        token_hash: &str,
        m: usize,
        public_keys: Vec<ECPoint>,
        keys: Vec<KeyPair>,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<(Transaction, String), RpcError> {
        let token_script_hash =
            RpcUtility::get_script_hash(token_hash, &self.rpc_client.protocol_settings)?;

        let tx = self
            .nep17_api
            .create_transfer_tx_multi_sig_with_assert(
                &token_script_hash,
                m,
                public_keys,
                keys,
                to,
                amount,
                data,
                add_assert,
            )
            .await?;

        let hash = self.rpc_client.send_raw_transaction(&tx).await?;
        Ok((tx, hash.to_string()))
   }

    /// Wait for a transaction to be confirmed.
    /// Matches C# `WaitTransactionAsync`
    pub async fn wait_transaction(&self, tx: &Transaction) -> Result<RpcTransaction, RpcError> {
        self.wait_transaction_with_timeout(tx, 60).await
   }

    /// Wait for a transaction to be confirmed with timeout in seconds.
    pub async fn wait_transaction_with_timeout(
        &self,
        tx: &Transaction,
        timeout_seconds: u64,
    ) -> Result<RpcTransaction, RpcError> {
        // Wait for transaction to be included in a block
        let tx_hash = tx.hash();
        let timeout = std::time::Duration::from_secs(timeout_seconds);
        let poll_interval =
            std::cmp::max(1, self.rpc_client.protocol_settings.milliseconds_per_block as u64 / 2);
        let poll_duration = tokio::time::Duration::from_millis(poll_interval);
        let deadline = std::time::Instant::now() + timeout;

        while std::time::Instant::now() < deadline {
            // Check if transaction is in a block
            if let Ok(rpc_tx) = self.rpc_client.get_transaction(&tx_hash.to_string()).await {
                if rpc_tx.confirmations.is_some() {
                    return Ok(rpc_tx);
               }
           } else {
                // Transaction not found yet, continue waiting
           }

            tokio::time::sleep(poll_duration).await;
       }

        Err(RpcError::Other(
            "Timeout while waiting for transaction confirmation".to_string(),
        ))
   }

    /// Get account state including balances
    /// Matches C# `GetAccountStateAsync`
    pub async fn get_account_state(&self, account: &str) -> Result<WalletAccountState, RpcError> {
        let account_hash =
            RpcUtility::get_script_hash(account, &self.rpc_client.protocol_settings)?;

        // Get NEO and GAS balances
        let neo_balance = self
            .nep17_api
            .balance_of(&neo_hash(), &account_hash)
            .await?;

        let gas_balance = self
            .nep17_api
            .balance_of(&gas_hash(), &account_hash)
            .await?;

        let unclaimed_gas = self.get_unclaimed_gas_from_hash(&account_hash).await?;

        Ok(WalletAccountState {
            address: WalletHelper::to_address(
                &account_hash,
                self.rpc_client.protocol_settings.address_version,
            ),
            neo_balance: neo_balance.to_u32().unwrap_or(0),
            gas_balance: gas_balance.to_f64().unwrap_or(0.0) / gas_factor() as f64,
            unclaimed_gas})
   }
}

fn neo_hash() -> UInt160 {
    NeoToken::new().hash()
}

fn gas_hash() -> UInt160 {
    GasToken::new().hash()
}

fn gas_factor() -> u64 {
    // GAS is a NEP-17 token with a fixed 8 decimals (C# `GasToken.Decimals => 8`).
    const GAS_DECIMALS: u8 = 8;
    10u64.saturating_pow(u32::from(GAS_DECIMALS))
}

/// Lightweight account snapshot returned by wallet RPC helpers
#[derive(Debug, Clone)]
pub struct WalletAccountState {
    /// Account address
    pub address: String,
    /// NEO balance
    pub neo_balance: u32,
    /// GAS balance
    pub gas_balance: f64,
    /// Unclaimed GAS
    pub unclaimed_gas: f64}

#[cfg(test)]
mod tests;
