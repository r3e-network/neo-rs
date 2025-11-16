// Copyright (C) 2015-2025 The Neo Project.
//
// nep17_api.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::models::{RpcNep17TokenInfo, RpcNep17Transfers};
use crate::{ContractClient, RpcClient, TransactionManagerFactory};
use neo_core::{Contract, KeyPair, Signer, Transaction, UInt160, WitnessScope};
use neo_vm::ScriptBuilder;
use num_bigint::BigInt;
use std::sync::Arc;

/// Call NEP17 methods with RPC API
/// Matches C# Nep17API
pub struct Nep17Api {
    /// Base contract client functionality
    contract_client: ContractClient,
    /// Direct access to RPC client
    rpc_client: Arc<RpcClient>,
}

impl Nep17Api {
    /// Nep17API Constructor
    /// Matches C# constructor
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self {
            contract_client: ContractClient::new(rpc_client.clone()),
            rpc_client,
        }
    }

    /// Get balance of NEP17 token
    /// Matches C# BalanceOfAsync
    pub async fn balance_of(
        &self,
        script_hash: &UInt160,
        account: &UInt160,
    ) -> Result<BigInt, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(script_hash, "balanceOf", vec![account.to_json()])
            .await?;

        // Get the single stack item and convert to integer
        let stack_item = result.stack.first().ok_or("No result returned")?;

        Ok(stack_item.get_integer()?)
    }

    /// Get symbol of NEP17 token
    /// Matches C# SymbolAsync
    pub async fn symbol(
        &self,
        script_hash: &UInt160,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(script_hash, "symbol", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        Ok(stack_item.get_string()?)
    }

    /// Get decimals of NEP17 token
    /// Matches C# DecimalsAsync
    pub async fn decimals(&self, script_hash: &UInt160) -> Result<u8, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(script_hash, "decimals", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let value = stack_item.get_integer()?;
        Ok(value.to_u8().ok_or("Invalid decimals value")?)
    }

    /// Get total supply of NEP17 token
    /// Matches C# TotalSupplyAsync
    pub async fn total_supply(
        &self,
        script_hash: &UInt160,
    ) -> Result<BigInt, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(script_hash, "totalSupply", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        Ok(stack_item.get_integer()?)
    }

    /// Get token information in one rpc call
    /// Matches C# GetTokenInfoAsync
    pub async fn get_token_info(
        &self,
        script_hash: &UInt160,
    ) -> Result<RpcNep17TokenInfo, Box<dyn std::error::Error>> {
        // Get contract state
        let contract_state = self
            .rpc_client
            .get_contract_state(&script_hash.to_string())
            .await?;

        // Build script to get all token info at once
        let mut script = Vec::new();
        script.extend(self.make_script(script_hash, "symbol", vec![])?);
        script.extend(self.make_script(script_hash, "decimals", vec![])?);
        script.extend(self.make_script(script_hash, "totalSupply", vec![])?);

        let name = contract_state.manifest.name.clone();
        let result = self.rpc_client.invoke_script(&script).await?;
        let stack = &result.stack;

        Ok(RpcNep17TokenInfo {
            name,
            symbol: stack
                .get(0)
                .and_then(|s| s.get_string().ok())
                .unwrap_or_default(),
            decimals: stack
                .get(1)
                .and_then(|s| s.get_integer().ok())
                .and_then(|i| i.to_u8())
                .unwrap_or(0),
            total_supply: stack
                .get(2)
                .and_then(|s| s.get_integer().ok())
                .unwrap_or_else(|| BigInt::from(0)),
        })
    }

    /// Get token information in one rpc call, including address info
    /// Matches C# GetTokenInfoAsync with address parameter
    pub async fn get_token_info_with_balance(
        &self,
        address: &str,
        script_hash: &UInt160,
    ) -> Result<RpcNep17TokenInfo, Box<dyn std::error::Error>> {
        let mut token_info = self.get_token_info(script_hash).await?;

        // Parse address to UInt160
        let account =
            UInt160::from_address(address, self.rpc_client.protocol_settings.address_version)?;

        // Get balance for the address
        let balance = self.balance_of(script_hash, &account).await?;
        token_info.balance = Some(balance);

        Ok(token_info)
    }

    /// Create NEP17 token transfer transaction
    /// Matches C# CreateTransferTxAsync
    pub async fn create_transfer_tx(
        &self,
        script_hash: &UInt160,
        key: &KeyPair,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        let from = Contract::create_signature_redeem_script(&key.public_key()).to_script_hash();

        self.create_transfer_tx_with_from(script_hash, &from, key, to, amount, data)
            .await
    }

    /// Create NEP17 token transfer transaction with specific from address
    /// Matches C# CreateTransferTxAsync with from parameter
    pub async fn create_transfer_tx_with_from(
        &self,
        script_hash: &UInt160,
        from: &UInt160,
        from_key: &KeyPair,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        // Build transfer script
        let mut sb = ScriptBuilder::new();

        // Emit transfer parameters in reverse order
        if let Some(d) = data {
            self.emit_argument(&mut sb, &d)?;
        } else {
            sb.emit_push_null()?;
        }
        sb.emit_push_int(amount.to_i64().ok_or("Amount too large")?)?;
        sb.emit_push(&to.to_array())?;
        sb.emit_push(&from.to_array())?;
        sb.emit_push_int(4)?; // Number of arguments
        sb.emit_push(b"transfer")?;
        sb.emit_push(&script_hash.to_array())?;
        sb.emit_syscall("System.Contract.Call")?;

        let script = sb.to_array();

        // Create signers
        let signers = vec![Signer {
            account: *from,
            scopes: WitnessScope::CALLED_BY_ENTRY,
            allowed_contracts: vec![],
            allowed_groups: vec![],
            rules: vec![],
        }];

        // Create and sign transaction
        let factory = TransactionManagerFactory::new(self.rpc_client.clone());
        let mut manager = factory.make_transaction(&script, &signers).await?;
        manager.add_signature(from_key)?;
        let transaction = manager.sign().await?;

        Ok(transaction)
    }

    /// Get NEP17 token transfers
    /// Matches C# GetNep17TransfersAsync
    pub async fn get_nep17_transfers(
        &self,
        address: &str,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> Result<RpcNep17Transfers, Box<dyn std::error::Error>> {
        self.rpc_client
            .get_nep17_transfers(address, start_time, end_time)
            .await
    }

    /// Get NEP17 token balances for an address
    /// Matches C# GetNep17BalancesAsync
    pub async fn get_nep17_balances(
        &self,
        address: &str,
    ) -> Result<Vec<RpcNep17TokenInfo>, Box<dyn std::error::Error>> {
        let balances = self.rpc_client.get_nep17_balances(address).await?;

        // Convert balances to token info
        let mut token_infos = Vec::new();
        for balance in balances.balances {
            let mut info = self.get_token_info(&balance.asset_hash).await?;
            info.balance = Some(balance.amount);
            info.last_updated_block = Some(balance.last_updated_block);
            token_infos.push(info);
        }

        Ok(token_infos)
    }

    // Helper methods

    fn make_script(
        &self,
        script_hash: &UInt160,
        operation: &str,
        args: Vec<serde_json::Value>,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut sb = ScriptBuilder::new();

        for arg in args.iter().rev() {
            self.emit_argument(&mut sb, arg)?;
        }

        sb.emit_push(operation.as_bytes())?;
        sb.emit_push(&script_hash.to_array())?;
        sb.emit_syscall("System.Contract.Call")?;

        Ok(sb.to_array())
    }

    fn emit_argument(
        &self,
        sb: &mut ScriptBuilder,
        arg: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match arg {
            serde_json::Value::Null => sb.emit_push_null(),
            serde_json::Value::Bool(b) => sb.emit_push(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    sb.emit_push_int(i)
                } else {
                    Err("Invalid number format".into())
                }
            }
            serde_json::Value::String(s) => sb.emit_push(s.as_bytes()),
            _ => Err("Unsupported argument type".into()),
        }
    }
}
