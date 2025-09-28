// Copyright (C) 2015-2025 The Neo Project.
//
// transaction_manager_factory.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::{RpcClient, TransactionManager};
use neo_core::{Signer, Transaction, TransactionAttribute};
use rand::Rng;
use std::sync::Arc;

/// Factory for creating TransactionManager instances
/// Matches C# TransactionManagerFactory
pub struct TransactionManagerFactory {
    /// The RPC client instance
    rpc_client: Arc<RpcClient>,
}

impl TransactionManagerFactory {
    /// TransactionManagerFactory Constructor
    /// Matches C# constructor
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self { rpc_client }
    }
    
    /// Create an unsigned Transaction object with given parameters
    /// Matches C# MakeTransactionAsync
    pub async fn make_transaction(
        &self,
        script: &[u8],
        signers: &[Signer],
    ) -> Result<TransactionManager, Box<dyn std::error::Error>> {
        // Invoke script to get gas consumption
        let invoke_result = self.rpc_client
            .invoke_script_with_signers(script, signers)
            .await?;
            
        self.make_transaction_with_fee(
            script,
            invoke_result.gas_consumed,
            signers,
            &[],
        ).await
    }
    
    /// Create an unsigned Transaction object with given parameters and attributes
    /// Matches C# MakeTransactionAsync with attributes
    pub async fn make_transaction_with_attributes(
        &self,
        script: &[u8],
        signers: &[Signer],
        attributes: &[TransactionAttribute],
    ) -> Result<TransactionManager, Box<dyn std::error::Error>> {
        // Invoke script to get gas consumption
        let invoke_result = self.rpc_client
            .invoke_script_with_signers(script, signers)
            .await?;
            
        self.make_transaction_with_fee(
            script,
            invoke_result.gas_consumed,
            signers,
            attributes,
        ).await
    }
    
    /// Create an unsigned Transaction object with given parameters and system fee
    /// Matches C# MakeTransactionAsync with systemFee parameter
    pub async fn make_transaction_with_fee(
        &self,
        script: &[u8],
        system_fee: i64,
        signers: &[Signer],
        attributes: &[TransactionAttribute],
    ) -> Result<TransactionManager, Box<dyn std::error::Error>> {
        // Get current block count
        let block_count = self.rpc_client.get_block_count().await?;
        
        // Generate random nonce
        let mut rng = rand::thread_rng();
        let nonce = rng.gen::<u32>();
        
        // Create transaction
        let tx = Transaction {
            version: 0,
            nonce,
            script: script.to_vec(),
            signers: signers.to_vec(),
            valid_until_block: block_count - 1 + self.rpc_client.protocol_settings.max_valid_until_block_increment,
            system_fee,
            network_fee: 0, // Will be calculated later
            attributes: attributes.to_vec(),
            witnesses: vec![],
        };
        
        Ok(TransactionManager::new(tx, self.rpc_client.clone()))
    }
}