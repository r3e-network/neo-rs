// Copyright (C) 2015-2025 The Neo Project.
// Licensed under the MIT License. See LICENSE file for details.

use super::{Signer, TransactionAttribute, Witness};
use neo_primitives::UInt256;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Maximum transaction size.
pub const MAX_TRANSACTION_SIZE: usize = 102400;
/// Maximum combined signer and attribute count on deserialization.
pub const MAX_TRANSACTION_ATTRIBUTES: usize = 16;
/// Version, nonce, system fee, network fee, and valid-until height widths.
pub const HEADER_SIZE: usize = 1 + 4 + 8 + 8 + 4;

/// Normative transaction data, without VM projection or snapshot verification.
#[derive(Debug, Serialize, Deserialize)]
pub struct Transaction {
    version: u8,
    nonce: u32,
    system_fee: i64,
    network_fee: i64,
    valid_until_block: u32,
    signers: Vec<Signer>,
    attributes: Vec<TransactionAttribute>,
    script: Vec<u8>,
    witnesses: Vec<Witness>,
    #[serde(skip)]
    _hash: Mutex<Option<UInt256>>,
    #[serde(skip)]
    _size: Mutex<Option<usize>>,
}
impl Clone for Transaction {
    fn clone(&self) -> Self {
        Self {
            version: self.version,
            nonce: self.nonce,
            system_fee: self.system_fee,
            network_fee: self.network_fee,
            valid_until_block: self.valid_until_block,
            signers: self.signers.clone(),
            attributes: self.attributes.clone(),
            script: self.script.clone(),
            witnesses: self.witnesses.clone(),
            _hash: Mutex::new(None),
            _size: Mutex::new(None),
        }
    }
}
mod core;
mod json;
mod serialization;
mod traits;
