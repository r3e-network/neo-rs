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

use crate::{Nep17Api, RpcClient, Utility};
use neo_core::{Contract, KeyPair, NativeContract, Signer, Transaction, UInt160, WitnessScope};
use num_bigint::BigInt;
use std::sync::Arc;

/// Wallet Common APIs
/// Matches C# WalletAPI
pub struct WalletApi {
    /// The RPC client instance
    rpc_client: Arc<RpcClient>,
    /// NEP17 API for token operations
    nep17_api: Nep17Api,
}

impl WalletApi {
    /// WalletAPI Constructor
    /// Matches C# constructor
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self {
            nep17_api: Nep17Api::new(rpc_client.clone()),
            rpc_client,
        }
    }

    /// Get unclaimed gas with address, scripthash or public key string
    /// Matches C# GetUnclaimedGasAsync with string parameter
    pub async fn get_unclaimed_gas(
        &self,
        account: &str,
    ) -> Result<f64, Box<dyn std::error::Error>> {
        let account_hash = Utility::get_script_hash(account, &self.rpc_client.protocol_settings)?;
        self.get_unclaimed_gas_from_hash(&account_hash).await
    }

    /// Get unclaimed gas
    /// Matches C# GetUnclaimedGasAsync with UInt160 parameter
    pub async fn get_unclaimed_gas_from_hash(
        &self,
        account: &UInt160,
    ) -> Result<f64, Box<dyn std::error::Error>> {
        let script_hash = NativeContract::neo().hash();
        let block_count = self.rpc_client.get_block_count().await?;

        let result = self
            .nep17_api
            .contract_client
            .test_invoke(
                &script_hash,
                "unclaimedGas",
                vec![account.to_json(), serde_json::json!(block_count - 1)],
            )
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let balance = stack_item.get_integer()?;
        let gas_factor = NativeContract::gas().factor();

        Ok(balance.to_f64().unwrap_or(0.0) / gas_factor as f64)
    }

    /// Get Neo Balance
    /// Matches C# GetNeoBalanceAsync
    pub async fn get_neo_balance(&self, account: &str) -> Result<u32, Box<dyn std::error::Error>> {
        let balance = self
            .get_token_balance(&NativeContract::neo().hash().to_string(), account)
            .await?;
        Ok(balance.to_u32().ok_or("Invalid NEO balance")?)
    }

    /// Get Gas Balance
    /// Matches C# GetGasBalanceAsync
    pub async fn get_gas_balance(&self, account: &str) -> Result<f64, Box<dyn std::error::Error>> {
        let balance = self
            .get_token_balance(&NativeContract::gas().hash().to_string(), account)
            .await?;
        let gas_factor = NativeContract::gas().factor();
        Ok(balance.to_f64().unwrap_or(0.0) / gas_factor as f64)
    }

    /// Get token balance with string parameters
    /// Matches C# GetTokenBalanceAsync
    pub async fn get_token_balance(
        &self,
        token_hash: &str,
        account: &str,
    ) -> Result<BigInt, Box<dyn std::error::Error>> {
        let token_script_hash = UInt160::parse(token_hash)?;
        let account_hash = Utility::get_script_hash(account, &self.rpc_client.protocol_settings)?;

        self.nep17_api
            .balance_of(&token_script_hash, &account_hash)
            .await
    }

    /// Claim GAS from NEO
    /// Matches C# ClaimGasAsync
    pub async fn claim_gas(
        &self,
        key: &KeyPair,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        let sender = Contract::create_signature_redeem_script(&key.public_key()).to_script_hash();

        self.claim_gas_from_account(&sender, key).await
    }

    /// Claim GAS from specific account
    /// Matches C# ClaimGasAsync with account parameter
    pub async fn claim_gas_from_account(
        &self,
        account: &UInt160,
        key: &KeyPair,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        let neo_balance = self
            .nep17_api
            .balance_of(&NativeContract::neo().hash(), account)
            .await?;

        if neo_balance == BigInt::from(0) {
            return Err("No NEO balance to claim GAS from".into());
        }

        // Transfer NEO to self to trigger GAS claim
        self.nep17_api
            .create_transfer_tx_with_from(
                &NativeContract::neo().hash(),
                account,
                key,
                account,
                neo_balance,
                None,
            )
            .await
    }

    /// Transfer NEP17 token
    /// Matches C# TransferAsync
    pub async fn transfer(
        &self,
        token_hash: &str,
        key: &KeyPair,
        to_address: &str,
        amount: BigInt,
        data: Option<serde_json::Value>,
    ) -> Result<(Transaction, String), Box<dyn std::error::Error>> {
        let token_script_hash = UInt160::parse(token_hash)?;
        let to = Utility::get_script_hash(to_address, &self.rpc_client.protocol_settings)?;

        let tx = self
            .nep17_api
            .create_transfer_tx(&token_script_hash, key, &to, amount, data)
            .await?;

        let tx_hash = tx.hash().to_string();
        Ok((tx, tx_hash))
    }

    /// Send transaction and wait for result
    /// Matches C# WaitTransactionAsync
    pub async fn wait_transaction(
        &self,
        tx: &Transaction,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // Send transaction
        let result = self.rpc_client.send_raw_transaction(tx).await?;

        if !result {
            return Ok(false);
        }

        // Wait for transaction to be included in a block
        let tx_hash = tx.hash();
        let timeout_ms = 60000; // 60 seconds timeout
        let poll_interval_ms = 1000; // Poll every second
        let mut elapsed_ms = 0;

        while elapsed_ms < timeout_ms {
            tokio::time::sleep(tokio::time::Duration::from_millis(poll_interval_ms)).await;
            elapsed_ms += poll_interval_ms;

            // Check if transaction is in a block
            match self.rpc_client.get_transaction(&tx_hash.to_string()).await {
                Ok(rpc_tx) => {
                    if rpc_tx.block_hash.is_some() {
                        return Ok(true);
                    }
                }
                Err(_) => {
                    // Transaction not found yet, continue waiting
                }
            }
        }

        Ok(false) // Timeout
    }

    /// Get account state including balances
    /// Matches C# GetAccountStateAsync
    pub async fn get_account_state(
        &self,
        account: &str,
    ) -> Result<AccountState, Box<dyn std::error::Error>> {
        let account_hash = Utility::get_script_hash(account, &self.rpc_client.protocol_settings)?;

        // Get NEO and GAS balances
        let neo_balance = self
            .nep17_api
            .balance_of(&NativeContract::neo().hash(), &account_hash)
            .await?;

        let gas_balance = self
            .nep17_api
            .balance_of(&NativeContract::gas().hash(), &account_hash)
            .await?;

        let unclaimed_gas = self.get_unclaimed_gas_from_hash(&account_hash).await?;

        Ok(AccountState {
            address: account_hash.to_address(self.rpc_client.protocol_settings.address_version),
            neo_balance: neo_balance.to_u32().unwrap_or(0),
            gas_balance: gas_balance.to_f64().unwrap_or(0.0)
                / NativeContract::gas().factor() as f64,
            unclaimed_gas,
        })
    }
}

/// Account state information
#[derive(Debug, Clone)]
pub struct AccountState {
    /// Account address
    pub address: String,
    /// NEO balance
    pub neo_balance: u32,
    /// GAS balance
    pub gas_balance: f64,
    /// Unclaimed GAS
    pub unclaimed_gas: f64,
}
