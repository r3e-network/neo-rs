// Copyright (C) 2015-2024 The Neo Project.
//
// HighPriorityAttribute.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::io::Write;
use crate::io::{BinaryReader, BinaryWriter};
use crate::network::payloads::Transaction;
use crate::network::transaction_attribute::{TransactionAttribute, TransactionAttributeType};
use crate::persistence::DataCache;
use crate::types::UInt160;
use crate::smart_contract::native::NativeContract;

/// Indicates that the transaction is of high priority.
#[derive(Default)]
pub struct HighPriorityAttribute;

impl TransactionAttribute for HighPriorityAttribute {
    fn allow_multiple(&self) -> bool {
        false
    }

    fn get_type(&self) -> TransactionAttributeType {
        TransactionAttributeType::HighPriority
    }

    fn deserialize_without_type(&mut self, _reader: &mut dyn BinaryReader) {
        // Empty implementation
    }

    fn serialize_without_type(&self, _writer: &mut dyn Write) {
        // Empty implementation
    }

    fn verify(&self, snapshot: &dyn DataCache, tx: &Transaction) -> bool {
        let committee = NativeContract::NEO.get_committee_address(snapshot);
        tx.signers.iter().any(|p| p.account == committee)
    }
}
