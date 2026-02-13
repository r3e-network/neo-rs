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

use crate::{RpcClient, RpcError, TransactionManager};
use neo_core::{Signer, Transaction, TransactionAttribute};
use rand::Rng;
use std::sync::Arc;

/// Factory for creating `TransactionManager` instances
/// Matches C# `TransactionManagerFactory`
pub struct TransactionManagerFactory {
    /// The RPC client instance
    rpc_client: Arc<RpcClient>,
}

impl TransactionManagerFactory {
    /// `TransactionManagerFactory` Constructor
    /// Matches C# constructor
    #[must_use]
    pub const fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self { rpc_client }
    }

    /// Create an unsigned Transaction object with given parameters
    /// Matches C# `MakeTransactionAsync`
    pub async fn make_transaction(
        &self,
        script: &[u8],
        signers: &[Signer],
    ) -> Result<TransactionManager, RpcError> {
        // Invoke script to get gas consumption
        let invoke_result = self
            .rpc_client
            .invoke_script_with_signers(script, signers)
            .await?;

        self.make_transaction_with_fee(script, invoke_result.gas_consumed, signers, &[])
            .await
    }

    /// Create an unsigned Transaction object with given parameters and attributes
    /// Matches C# `MakeTransactionAsync` with attributes
    pub async fn make_transaction_with_attributes(
        &self,
        script: &[u8],
        signers: &[Signer],
        attributes: &[TransactionAttribute],
    ) -> Result<TransactionManager, RpcError> {
        // Invoke script to get gas consumption
        let invoke_result = self
            .rpc_client
            .invoke_script_with_signers(script, signers)
            .await?;

        self.make_transaction_with_fee(script, invoke_result.gas_consumed, signers, attributes)
            .await
    }

    /// Create an unsigned Transaction object with given parameters and system fee
    /// Matches C# `MakeTransactionAsync` with systemFee parameter
    pub async fn make_transaction_with_fee(
        &self,
        script: &[u8],
        system_fee: i64,
        signers: &[Signer],
        attributes: &[TransactionAttribute],
    ) -> Result<TransactionManager, RpcError> {
        // Get current block count (RPC returns height + 1)
        let block_count = self.rpc_client.get_block_count().await?;
        let current_height = block_count.saturating_sub(1);

        // Generate random nonce
        let mut rng = rand::thread_rng();
        let nonce = rng.gen::<u32>();

        // Create transaction
        let mut tx = Transaction::new();
        tx.set_nonce(nonce);
        tx.set_script(script.to_vec());
        tx.set_signers(signers.to_vec());
        tx.set_valid_until_block(
            current_height.saturating_sub(1).saturating_add(
                self.rpc_client
                    .protocol_settings
                    .max_valid_until_block_increment,
            ),
        );
        tx.set_system_fee(system_fee);
        tx.set_attributes(attributes.to_vec());
        tx.set_witnesses(Vec::new());

        Ok(TransactionManager::new(tx, self.rpc_client.clone()))
    }
}
