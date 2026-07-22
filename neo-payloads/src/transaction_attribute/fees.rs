//! Transaction attribute network-fee calculation.

use super::TransactionAttribute;
use crate::transaction::Transaction;

impl TransactionAttribute {
    /// Calculate the network fee for this attribute.
    /// Matches C# CalculateNetworkFee method.
    pub fn calculate_network_fee(&self, base: i64, tx: &Transaction) -> i64 {
        match self {
            Self::Conflicts(_) => tx.signers().len() as i64 * base,
            Self::NotaryAssisted(attr) => (i64::from(attr.nkeys) + 1) * base,
            _ => base,
        }
    }
}
