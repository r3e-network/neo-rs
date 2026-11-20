//! Canonical network-fee policy for transaction attributes.
//!
//! Mirrors `neo-core`'s `TransactionAttribute::calculate_network_fee`: the
//! `PolicyContract` per-type attribute fee, scaled by the signer count for
//! `Conflicts` and by `nkeys + 1` for `NotaryAssisted`.
//!
//! This module is the single owner of that policy. Fee estimators
//! (`neo-wallets`) and transaction verification (`neo-ledger`) both delegate
//! here, so a transaction is charged exactly what validation enforces.

use super::PolicyContract;
use neo_protocol::payloads::{Transaction, TransactionAttribute};
use neo_storage::StorageItem;
use neo_storage::StorageKey;
use neo_storage::persistence::read_only_store::ReadOnlyStoreGeneric;

/// Computes the network fee contributed by a single transaction attribute.
///
/// The base fee is read from the policy contract for the attribute type,
/// defaulting to [`PolicyContract::DEFAULT_ATTRIBUTE_FEE`] when the snapshot has
/// no stored value (or the type is unknown). `Conflicts` charges the base fee
/// once per signer and `NotaryAssisted` once per notary key plus the payer.
pub fn attribute_network_fee<S>(
    attribute: &TransactionAttribute,
    snapshot: &S,
    transaction: &Transaction,
) -> i64
where
    S: ReadOnlyStoreGeneric<StorageKey, StorageItem>,
{
    let policy = PolicyContract::new();
    let base_fee = policy
        .get_attribute_fee_snapshot(snapshot, attribute.type_id())
        .unwrap_or(PolicyContract::DEFAULT_ATTRIBUTE_FEE as i64);

    match attribute {
        TransactionAttribute::Conflicts(_) => transaction.signers().len() as i64 * base_fee,
        TransactionAttribute::NotaryAssisted(attribute) => (attribute.nkeys as i64 + 1) * base_fee,
        _ => base_fee,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::{UInt160, UInt256, WitnessScope};
    use neo_protocol::payloads::{Conflicts, NotaryAssisted, Signer};
    use neo_storage::persistence::DataCache;

    fn empty_snapshot() -> DataCache {
        DataCache::new(true)
    }

    #[test]
    fn default_attribute_fee_applies_without_policy_storage() {
        let snapshot = empty_snapshot();
        let attribute = TransactionAttribute::HighPriority;
        let transaction = Transaction::new();
        assert_eq!(
            attribute_network_fee(&attribute, &snapshot, &transaction),
            PolicyContract::DEFAULT_ATTRIBUTE_FEE as i64
        );
    }

    #[test]
    fn notary_assisted_scales_by_nkeys_plus_one() {
        let snapshot = empty_snapshot();
        let attribute = TransactionAttribute::NotaryAssisted(NotaryAssisted { nkeys: 3 });
        let transaction = Transaction::new();
        let base = PolicyContract::DEFAULT_ATTRIBUTE_FEE as i64;
        assert_eq!(
            attribute_network_fee(&attribute, &snapshot, &transaction),
            4 * base
        );
    }

    #[test]
    fn conflicts_attribute_scales_by_signer_count() {
        let snapshot = empty_snapshot();
        let attribute = TransactionAttribute::Conflicts(Conflicts {
            hash: UInt256::zero(),
        });
        let base = PolicyContract::new()
            .get_attribute_fee_snapshot(&snapshot, attribute.type_id())
            .unwrap_or(PolicyContract::DEFAULT_ATTRIBUTE_FEE as i64);

        let mut transaction = Transaction::new();
        assert_eq!(
            attribute_network_fee(&attribute, &snapshot, &transaction),
            0
        );

        transaction.add_signer(Signer::new(UInt160::zero(), WitnessScope::CALLED_BY_ENTRY));
        transaction.add_signer(Signer::new(UInt160::zero(), WitnessScope::CALLED_BY_ENTRY));
        assert_eq!(
            attribute_network_fee(&attribute, &snapshot, &transaction),
            2 * base
        );
    }
}
