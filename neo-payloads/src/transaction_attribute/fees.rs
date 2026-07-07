//! Transaction attribute network-fee calculation.

use neo_storage::{DataCache, StorageKey};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use super::TransactionAttribute;
use crate::{TransactionAttributeType, transaction::Transaction};

const POLICY_CONTRACT_ID: i32 = -7;
const POLICY_PREFIX_ATTRIBUTE_FEE: u8 = 20;
const DEFAULT_ATTRIBUTE_FEE: i64 = 0;

impl TransactionAttribute {
    /// Calculate the network fee for this attribute.
    /// Matches C# CalculateNetworkFee method.
    pub fn calculate_network_fee(&self, snapshot: &DataCache, tx: &Transaction) -> i64 {
        let base = policy_attribute_fee(snapshot, self.type_id());
        match self {
            Self::Conflicts(_) => tx.signers().len() as i64 * base,
            Self::NotaryAssisted(attr) => (i64::from(attr.nkeys) + 1) * base,
            _ => base,
        }
    }
}

fn policy_attribute_fee(snapshot: &DataCache, attribute_type: TransactionAttributeType) -> i64 {
    let key = StorageKey::new(
        POLICY_CONTRACT_ID,
        vec![POLICY_PREFIX_ATTRIBUTE_FEE, attribute_type.to_byte()],
    );
    snapshot
        .get(&key)
        .and_then(|item| BigInt::from_signed_bytes_le(&item.value_bytes()).to_i64())
        .unwrap_or(DEFAULT_ATTRIBUTE_FEE)
}
