// Copyright (C) 2015-2025 The Neo Project.
//
// contains_transaction_type.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_primitives::protocol_enum;

protocol_enum! {
    /// Represents the type of transaction containment.
    /// Matches C# ContainsTransactionType enum exactly.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub ContainsTransactionType {
        /// Transaction does not exist.
        /// Matches C# NotExist variant.
        NotExist = 0,
        /// Transaction exists in the memory pool.
        /// Matches C# ExistsInPool variant.
        ExistsInPool = 1,
        /// Transaction exists in the ledger.
        /// Matches C# ExistsInLedger variant.
        ExistsInLedger = 2,
    }
}

#[cfg(test)]
mod tests {
    use super::ContainsTransactionType;

    #[test]
    fn contains_transaction_type_matches_neo_values() {
        assert_eq!(ContainsTransactionType::NotExist.to_byte(), 0);
        assert_eq!(ContainsTransactionType::ExistsInPool.to_byte(), 1);
        assert_eq!(ContainsTransactionType::ExistsInLedger.to_byte(), 2);

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
    fn contains_transaction_type_preserves_existing_debug_names() {
        assert_eq!(
            format!("{:?}", ContainsTransactionType::NotExist),
            "NotExist"
        );
        assert_eq!(
            format!("{:?}", ContainsTransactionType::ExistsInPool),
            "ExistsInPool"
        );
        assert_eq!(
            format!("{:?}", ContainsTransactionType::ExistsInLedger),
            "ExistsInLedger"
        );
    }
}
