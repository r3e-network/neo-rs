//! ContainsTransactionType - matches C# Neo.Ledger.ContainsTransactionType exactly.

use neo_primitives::protocol_enum;

protocol_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    /// Represents the type of transaction containment.
    pub ContainsTransactionType {
        NotExist = 0,
        ExistsInPool = 1,
        ExistsInLedger = 2,
    }
}

impl ContainsTransactionType {
    /// Returns true if the transaction exists (either in pool or ledger).
    pub fn exists(self) -> bool {
        !matches!(self, Self::NotExist)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains_transaction_type_values() {
        assert_eq!(ContainsTransactionType::NotExist.to_byte(), 0);
        assert_eq!(ContainsTransactionType::ExistsInPool.to_byte(), 1);
        assert_eq!(ContainsTransactionType::ExistsInLedger.to_byte(), 2);
    }

    #[test]
    fn test_contains_transaction_type_from_byte() {
        assert_eq!(
            ContainsTransactionType::from_byte(0),
            Some(ContainsTransactionType::NotExist)
        );
        assert_eq!(
            ContainsTransactionType::from_byte(1),
            Some(ContainsTransactionType::ExistsInPool)
        );
        assert_eq!(
            ContainsTransactionType::from_byte(2),
            Some(ContainsTransactionType::ExistsInLedger)
        );
        assert_eq!(ContainsTransactionType::from_byte(3), None);
    }

    #[test]
    fn test_contains_transaction_type_roundtrip() {
        for ctt in [
            ContainsTransactionType::NotExist,
            ContainsTransactionType::ExistsInPool,
            ContainsTransactionType::ExistsInLedger,
        ] {
            let byte = ctt.to_byte();
            let recovered = ContainsTransactionType::from_byte(byte);
            assert_eq!(recovered, Some(ctt));
        }
    }

    #[test]
    fn test_contains_transaction_type_display() {
        assert_eq!(ContainsTransactionType::NotExist.to_string(), "NotExist");
        assert_eq!(
            ContainsTransactionType::ExistsInPool.to_string(),
            "ExistsInPool"
        );
        assert_eq!(
            ContainsTransactionType::ExistsInLedger.to_string(),
            "ExistsInLedger"
        );
    }

    #[test]
    fn test_contains_transaction_type_exists() {
        assert!(!ContainsTransactionType::NotExist.exists());
        assert!(ContainsTransactionType::ExistsInPool.exists());
        assert!(ContainsTransactionType::ExistsInLedger.exists());
    }

    #[test]
    fn test_contains_transaction_type_serde() {
        let ctt = ContainsTransactionType::ExistsInPool;
        let serialized = serde_json::to_string(&ctt).unwrap();
        assert_eq!(serialized, "1");

        let deserialized: ContainsTransactionType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, ctt);
    }
}
