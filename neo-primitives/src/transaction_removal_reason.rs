//! `TransactionRemovalReason` - matches C# Neo.Ledger.TransactionRemovalReason exactly.
//!
//! This is the single source of truth for transaction removal reasons. Both
//! `neo-core::ledger` and `neo-p2p` re-export this type for backward compatibility.

use crate::protocol_enum;

protocol_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    /// The reason a transaction was removed from the memory pool.
    pub TransactionRemovalReason {
        /// The transaction was rejected since it was the lowest priority transaction
        /// and the memory pool capacity was exceeded.
        CapacityExceeded = 0,
        /// The transaction was rejected due to failing re-validation after a block was persisted.
        NoLongerValid = 1,
        /// The transaction was rejected due to conflict with higher priority transactions
        /// with Conflicts attribute.
        Conflict = 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_removal_reason_values_match_protocol_bytes() {
        assert_eq!(TransactionRemovalReason::CapacityExceeded.to_byte(), 0);
        assert_eq!(TransactionRemovalReason::NoLongerValid.to_byte(), 1);
        assert_eq!(TransactionRemovalReason::Conflict.to_byte(), 2);
    }

    #[test]
    fn transaction_removal_reason_from_byte_rejects_unknown_values() {
        assert_eq!(
            TransactionRemovalReason::from_byte(0),
            Some(TransactionRemovalReason::CapacityExceeded)
        );
        assert_eq!(
            TransactionRemovalReason::from_byte(1),
            Some(TransactionRemovalReason::NoLongerValid)
        );
        assert_eq!(
            TransactionRemovalReason::from_byte(2),
            Some(TransactionRemovalReason::Conflict)
        );
        assert_eq!(TransactionRemovalReason::from_byte(3), None);
        assert_eq!(TransactionRemovalReason::from_byte(255), None);
    }

    #[test]
    fn transaction_removal_reason_roundtrips_protocol_bytes() {
        for reason in [
            TransactionRemovalReason::CapacityExceeded,
            TransactionRemovalReason::NoLongerValid,
            TransactionRemovalReason::Conflict,
        ] {
            assert_eq!(
                TransactionRemovalReason::from_byte(reason.to_byte()),
                Some(reason)
            );
        }
    }

    #[test]
    fn transaction_removal_reason_display_matches_variant_names() {
        assert_eq!(
            TransactionRemovalReason::CapacityExceeded.to_string(),
            "CapacityExceeded"
        );
        assert_eq!(
            TransactionRemovalReason::NoLongerValid.to_string(),
            "NoLongerValid"
        );
        assert_eq!(TransactionRemovalReason::Conflict.to_string(), "Conflict");
    }

    #[test]
    fn transaction_removal_reason_serde_uses_protocol_byte() {
        let reason = TransactionRemovalReason::NoLongerValid;
        let serialized = serde_json::to_string(&reason).unwrap();
        assert_eq!(serialized, "1");

        let deserialized: TransactionRemovalReason = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, reason);
    }

    #[test]
    fn transaction_removal_reason_serde_rejects_unknown_values() {
        assert!(serde_json::from_str::<TransactionRemovalReason>("3").is_err());
        assert!(serde_json::from_str::<TransactionRemovalReason>("255").is_err());
    }
}
