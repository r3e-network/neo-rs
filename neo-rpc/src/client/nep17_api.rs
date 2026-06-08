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

use super::contract_script::{build_dynamic_call_script, emit_contract_call};
use super::models::{RpcContractState, RpcNep17TokenInfo, RpcNep17Transfers};
use crate::{ContractClient, RpcClient, RpcError, RpcUtility, TransactionManagerFactory};
use neo_script_builder::ScriptBuilder;
use neo_manifest::CallFlags;
use neo_wallets::Helper as WalletHelper;
use neo_payloads::{Signer, Transaction};
use neo_crypto::{ECPoint, KeyPair};
use neo_execution::{Contract};
use neo_primitives::{UInt160, WitnessScope};
use neo_vm_rs::OpCode;
use num_bigint::BigInt;
use num_traits::cast::ToPrimitive;
use std::sync::Arc;

/// Call NEP17 methods with RPC API
/// Matches C# `Nep17API`
pub struct Nep17Api {
    /// Base contract client functionality
    contract_client: ContractClient,
    /// Direct access to RPC client
    rpc_client: Arc<RpcClient>}

impl Nep17Api {
    /// `Nep17API` Constructor
    /// Matches C# constructor
    #[must_use]
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self {
            contract_client: ContractClient::new(rpc_client.clone()),
            rpc_client}
   }

    /// Exposes the underlying contract client for advanced scenarios.
    #[must_use]
    pub const fn contract_client(&self) -> &ContractClient {
        &self.contract_client
   }

    /// Get balance of NEP17 token
    /// Matches C# `BalanceOfAsync`
    pub async fn balance_of(
        &self,
        script_hash: &UInt160,
        account: &UInt160,
    ) -> Result<BigInt, RpcError> {
        let result = self
            .contract_client
            .test_invoke(
                script_hash,
                "balanceOf",
                vec![serde_json::json!(account.to_string())],
            )
            .await?;

        // Get the single stack item and convert to integer
        let stack_item = result.stack.first().ok_or("No result returned")?;

        Ok(RpcUtility::stack_value_to_bigint(stack_item)?)
   }

    /// Get symbol of NEP17 token
    /// Matches C# `SymbolAsync`
    pub async fn symbol(&self, script_hash: &UInt160) -> Result<String, RpcError> {
        let result = self
            .contract_client
            .test_invoke(script_hash, "symbol", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        Ok(RpcUtility::stack_value_to_string(stack_item)?)
   }

    /// Get decimals of NEP17 token
    /// Matches C# `DecimalsAsync`
    pub async fn decimals(&self, script_hash: &UInt160) -> Result<u8, RpcError> {
        let result = self
            .contract_client
            .test_invoke(script_hash, "decimals", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let value = RpcUtility::stack_value_to_bigint(stack_item)?;
        Ok(value.to_u8().ok_or("Invalid decimals value")?)
   }

    /// Get total supply of NEP17 token
    /// Matches C# `TotalSupplyAsync`
    pub async fn total_supply(&self, script_hash: &UInt160) -> Result<BigInt, RpcError> {
        let result = self
            .contract_client
            .test_invoke(script_hash, "totalSupply", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        Ok(RpcUtility::stack_value_to_bigint(stack_item)?)
   }

    /// Get token information in one rpc call
    /// Matches C# `GetTokenInfoAsync`
    pub async fn get_token_info(
        &self,
        script_hash: &UInt160,
    ) -> Result<RpcNep17TokenInfo, RpcError> {
        let contract_state = self
            .rpc_client
            .get_contract_state(&script_hash.to_string())
            .await?;

        self.token_info_from_state(contract_state).await
   }

    /// Get token information for a contract hash or name.
    /// Matches C# GetTokenInfoAsync(string contractHash)
    pub async fn get_token_info_by_contract(
        &self,
        contract: &str,
    ) -> Result<RpcNep17TokenInfo, RpcError> {
        let contract_state = self.rpc_client.get_contract_state(contract).await?;

        self.token_info_from_state(contract_state).await
   }

    /// Get token information in one rpc call, including address info
    /// Matches C# `GetTokenInfoAsync` with address parameter
    pub async fn get_token_info_with_balance(
        &self,
        address: &str,
        script_hash: &UInt160,
    ) -> Result<RpcNep17TokenInfo, RpcError> {
        let mut token_info = self.get_token_info(script_hash).await?;

        // Parse address to UInt160 using the client's address version.
        let account = if let Ok(hash) = UInt160::parse(address) {
            hash
       } else {
            WalletHelper::to_script_hash(address, self.rpc_client.protocol_settings.address_version)
                .map_err(|e| RpcError::Other(e.to_string()))?
       };

        // Get balance for the address
        let balance = self.balance_of(script_hash, &account).await?;
        token_info.balance = Some(balance);

        Ok(token_info)
   }

    /// Create NEP17 token transfer transaction
    /// Matches C# `CreateTransferTxAsync`
    pub async fn create_transfer_tx(
        &self,
        script_hash: &UInt160,
        key: &KeyPair,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
    ) -> Result<Transaction, RpcError> {
        self.create_transfer_tx_with_assert(script_hash, key, to, amount, data, true)
            .await
   }

    /// Create NEP17 token transfer transaction with specific from address
    /// Matches C# `CreateTransferTxAsync` with from parameter
    pub async fn create_transfer_tx_with_from(
        &self,
        script_hash: &UInt160,
        from: &UInt160,
        from_key: &KeyPair,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
    ) -> Result<Transaction, RpcError> {
        self.create_transfer_tx_with_from_and_assert(
            script_hash,
            from,
            from_key,
            to,
            amount,
            data,
            true,
        )
        .await
   }

    /// Create NEP17 token transfer transaction with optional assert emission.
    pub async fn create_transfer_tx_with_assert(
        &self,
        script_hash: &UInt160,
        key: &KeyPair,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<Transaction, RpcError> {
        let from_script = Contract::create_signature_redeem_script(key.get_public_key_point()?);
        let from = UInt160::from_script(&from_script);
        self.create_transfer_tx_with_from_and_assert(
            script_hash,
            &from,
            key,
            to,
            amount,
            data,
            add_assert,
        )
        .await
   }

    #[allow(clippy::too_many_arguments)]
    /// Create NEP17 token transfer transaction with explicit from and optional assert.
    pub async fn create_transfer_tx_with_from_and_assert(
        &self,
        script_hash: &UInt160,
        from: &UInt160,
        from_key: &KeyPair,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<Transaction, RpcError> {
        let script =
            self.build_transfer_script(script_hash, from, to, &amount, data, add_assert)?;

        // Create signers
        let signers = vec![Signer {
            account: *from,
            scopes: WitnessScope::CALLED_BY_ENTRY,
            allowed_contracts: vec![],
            allowed_groups: vec![],
            rules: vec![]}];

        // Create and sign transaction
        let factory = TransactionManagerFactory::new(self.rpc_client.clone());
        let mut manager = factory.make_transaction(&script, &signers).await?;
        manager.add_signature(from_key)?;
        let transaction = manager.sign().await?;

        Ok(transaction)
   }

    #[allow(clippy::too_many_arguments)]
    /// Create NEP17 token transfer transaction from multi-sig account.
    /// Matches C# `CreateTransferTxAsync` with multi-sig overload.
    pub async fn create_transfer_tx_multi_sig(
        &self,
        script_hash: &UInt160,
        m: usize,
        public_keys: Vec<ECPoint>,
        from_keys: Vec<KeyPair>,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
    ) -> Result<Transaction, RpcError> {
        self.create_transfer_tx_multi_sig_with_assert(
            script_hash,
            m,
            public_keys,
            from_keys,
            to,
            amount,
            data,
            true,
        )
        .await
   }

    #[allow(clippy::too_many_arguments)]
    /// Create NEP17 token transfer transaction from multi-sig account with optional assert.
    pub async fn create_transfer_tx_multi_sig_with_assert(
        &self,
        script_hash: &UInt160,
        m: usize,
        public_keys: Vec<ECPoint>,
        from_keys: Vec<KeyPair>,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<Transaction, RpcError> {
        if m > from_keys.len() {
            return Err(format!("Need at least {m} KeyPairs for signing!").into());
       }

        let sender = Contract::create_multi_sig_contract(m, &public_keys).script_hash();
        let script =
            self.build_transfer_script(script_hash, &sender, to, &amount, data, add_assert)?;

        let signers = vec![Signer {
            account: sender,
            scopes: WitnessScope::CALLED_BY_ENTRY,
            allowed_contracts: vec![],
            allowed_groups: vec![],
            rules: vec![]}];

        let factory = TransactionManagerFactory::new(self.rpc_client.clone());
        let mut manager = factory.make_transaction(&script, &signers).await?;
        manager.add_multi_sig_with_keys(from_keys, m, public_keys)?;
        let transaction = manager.sign().await?;

        Ok(transaction)
   }

    /// Get NEP17 token transfers
    /// Matches C# `GetNep17TransfersAsync`
    pub async fn get_nep17_transfers(
        &self,
        address: &str,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> Result<RpcNep17Transfers, RpcError> {
        Ok(self
            .rpc_client
            .get_nep17_transfers(address, start_time, end_time)
            .await?)
   }

    /// Get NEP17 token balances for an address
    /// Matches C# `GetNep17BalancesAsync`
    pub async fn get_nep17_balances(
        &self,
        address: &str,
    ) -> Result<Vec<RpcNep17TokenInfo>, RpcError> {
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
    fn build_transfer_script(
        &self,
        script_hash: &UInt160,
        from: &UInt160,
        to: &UInt160,
        amount: &BigInt,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<Vec<u8>, RpcError> {
        let mut sb = ScriptBuilder::new();

        // Emit transfer parameters in reverse order.
        if let Some(d) = data {
            self.emit_argument(&mut sb, &d)?;
       } else {
            sb.emit_opcode(OpCode::PUSHNULL);
       }
        sb.emit_push_int(amount.to_i64().ok_or("Amount too large")?);
        sb.emit_push(&to.to_array());
        sb.emit_push(&from.to_array());
        sb.emit_push_int(4);
        emit_contract_call(&mut sb, script_hash, "transfer", CallFlags::ALL)?;
        if add_assert {
            sb.emit_opcode(OpCode::ASSERT);
       }

        Ok(sb.to_array())
   }

    async fn token_info_from_state(
        &self,
        contract_state: RpcContractState,
    ) -> Result<RpcNep17TokenInfo, RpcError> {
        let contract_hash = &contract_state.contract_state.hash;
        let name = contract_state.contract_state.manifest.name.clone();

        // Build script to get all token info at once.
        let mut script = Vec::new();
        script.extend(self.make_script(contract_hash, "symbol", vec![])?);
        script.extend(self.make_script(contract_hash, "decimals", vec![])?);
        script.extend(self.make_script(contract_hash, "totalSupply", vec![])?);

        let result = self.rpc_client.invoke_script(&script).await?;
        let stack = &result.stack;

        Ok(RpcNep17TokenInfo {
            name,
            symbol: stack
                .first()
                .and_then(|s| RpcUtility::stack_value_to_string(s).ok())
                .unwrap_or_default(),
            decimals: stack
                .get(1)
                .and_then(|s| RpcUtility::stack_value_to_bigint(s).ok())
                .and_then(|i| i.to_u8())
                .unwrap_or(0),
            total_supply: stack
                .get(2)
                .and_then(|s| RpcUtility::stack_value_to_bigint(s).ok())
                .unwrap_or_else(|| BigInt::from(0)),
            balance: None,
            last_updated_block: None})
   }

    fn make_script(
        &self,
        script_hash: &UInt160,
        operation: &str,
        args: Vec<serde_json::Value>,
    ) -> Result<Vec<u8>, RpcError> {
        build_dynamic_call_script(script_hash, operation, &args, CallFlags::ALL)
   }

    fn emit_argument(
        &self,
        sb: &mut ScriptBuilder,
        arg: &serde_json::Value,
    ) -> Result<(), RpcError> {
        match arg {
            serde_json::Value::Null => {
                sb.emit_opcode(OpCode::PUSHNULL);
                Ok(())
           }
            serde_json::Value::Bool(b) => {
                sb.emit_push_bool(*b);
                Ok(())
           }
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    sb.emit_push_int(i);
                    Ok(())
               } else {
                    Err("Invalid number format".into())
               }
           }
            serde_json::Value::String(s) => {
                sb.emit_push(s.as_bytes());
                Ok(())
           }
            serde_json::Value::Array(arr) => {
                for item in arr.iter().rev() {
                    self.emit_argument(sb, item)?;
               }
                sb.emit_push_int(arr.len() as i64);
                sb.emit_pack();
                Ok(())
           }
            _ => Err("Unsupported argument type".into())}
   }
}

#[cfg(test)]
mod tests;
