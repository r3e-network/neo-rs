// Copyright (C) 2015-2025 The Neo Project.
//
// block.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    header::Header, i_inventory::IInventory, i_verifiable::IVerifiable,
    inventory_type::InventoryType, transaction::Transaction, witness::Witness,
};
use crate::ledger::HeaderCache;
use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::persistence::{DataCache, StoreCache};
use crate::protocol_settings::ProtocolSettings;
use crate::{UInt160, UInt256};
use serde::{Deserialize, Serialize};

/// Represents a block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// The header of the block.
    pub header: Header,

    /// The transaction list of the block.
    pub transactions: Vec<Transaction>,
}

impl Block {
    /// Creates a new block.
    pub fn new() -> Self {
        Self {
            header: Header::new(),
            transactions: Vec::new(),
        }
    }

    /// Gets the hash of the block.
    pub fn hash(&mut self) -> UInt256 {
        self.header.hash()
    }

    /// Gets the version of the block.
    pub fn version(&self) -> u32 {
        self.header.version()
    }

    /// Gets the hash of the previous block.
    pub fn prev_hash(&self) -> &UInt256 {
        self.header.prev_hash()
    }

    /// Gets the merkle root of the transactions.
    pub fn merkle_root(&self) -> &UInt256 {
        self.header.merkle_root()
    }

    /// Gets the timestamp of the block.
    pub fn timestamp(&self) -> u64 {
        self.header.timestamp()
    }

    /// Gets the nonce of the block.
    pub fn nonce(&self) -> u64 {
        self.header.nonce()
    }

    /// Gets the index of the block.
    pub fn index(&self) -> u32 {
        self.header.index()
    }

    /// Gets the primary index of the consensus node.
    pub fn primary_index(&self) -> u8 {
        self.header.primary_index()
    }

    /// Gets the next consensus address.
    pub fn next_consensus(&self) -> &UInt160 {
        self.header.next_consensus()
    }

    /// Gets the witness of the block.
    pub fn witness(&self) -> &Witness {
        &self.header.witness
    }

    /// Calculates the network fee for the block.
    pub fn calculate_network_fee(&self, _snapshot: &DataCache) -> i64 {
        // Sum of all transaction network fees
        self.transactions.iter().map(|tx| tx.network_fee()).sum()
    }

    /// Rebuilds the merkle root.
    pub fn rebuild_merkle_root(&mut self) {
        if self.transactions.is_empty() {
            self.header.set_merkle_root(UInt256::default());
            return;
        }
        let payload_hashes: Vec<UInt256> =
            self.transactions.iter_mut().map(|tx| tx.hash()).collect();
        if let Some(root) = crate::neo_cryptography::MerkleTree::compute_root(&payload_hashes) {
            self.header.set_merkle_root(root);
        }
    }

    /// Verifies the block using persisted state.
    pub fn verify(&self, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool {
        self.header.verify(settings, store_cache)
    }

    /// Verifies the block using persisted state and cached headers.
    pub fn verify_with_cache(
        &self,
        settings: &ProtocolSettings,
        store_cache: &StoreCache,
        header_cache: &HeaderCache,
    ) -> bool {
        self.header
            .verify_with_cache(settings, store_cache, header_cache)
    }
}

impl IInventory for Block {
    fn inventory_type(&self) -> InventoryType {
        InventoryType::Block
    }

    fn hash(&mut self) -> UInt256 {
        self.header.hash()
    }
}

impl IVerifiable for Block {
    fn get_script_hashes_for_verifying(&self, snapshot: &DataCache) -> Vec<UInt160> {
        self.header.get_script_hashes_for_verifying(snapshot)
    }

    fn get_witnesses(&self) -> Vec<&Witness> {
        self.header.get_witnesses()
    }

    fn get_witnesses_mut(&mut self) -> Vec<&mut Witness> {
        self.header.get_witnesses_mut()
    }
}

impl Serializable for Block {
    fn size(&self) -> usize {
        self.header.size()
            + get_var_size(self.transactions.len() as u64)
            + self.transactions.iter().map(|tx| tx.size()).sum::<usize>()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&self.header, writer)?;

        const MAX_TRANSACTIONS: u64 = u16::MAX as u64;
        if self.transactions.len() as u64 > MAX_TRANSACTIONS {
            return Err(IoError::invalid_data("Too many transactions"));
        }
        writer.write_var_uint(self.transactions.len() as u64)?;

        // Write transactions
        for tx in &self.transactions {
            writer.write_serializable(tx)?;
        }

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let header = <Header as Serializable>::deserialize(reader)?;

        // Read transaction count
        const MAX_TRANSACTIONS: u64 = u16::MAX as u64;
        let tx_count = reader.read_var_int(MAX_TRANSACTIONS)? as usize;
        if tx_count as u64 > MAX_TRANSACTIONS {
            return Err(IoError::invalid_data("Too many transactions"));
        }

        let mut transactions = Vec::with_capacity(tx_count as usize);
        for _ in 0..tx_count {
            transactions.push(<Transaction as Serializable>::deserialize(reader)?);
        }

        Ok(Self {
            header,
            transactions,
        })
    }
}

impl Default for Block {
    fn default() -> Self {
        Self::new()
    }
}
