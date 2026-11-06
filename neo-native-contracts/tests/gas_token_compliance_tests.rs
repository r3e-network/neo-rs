//
// Migrated from neo-core/tests/gas_token_compliance_tests.rs (Task #228 wave 1).
// Test names and bodies preserved verbatim; import roots repointed to their
// owning crates (neo-native-contracts / neo-smart-contract / neo-protocol /
// neo-storage / neo-primitives / neo-vm / neo-crypto / neo-wallets).
// create_genesis_block here returns neo_protocol::payloads::Block directly, so the
// former genesis.to_payload_block() calls became genesis.clone().
//
//! GasToken C# compliance tests

#[cfg(test)]
mod tests {
    use neo_native_contracts::{GasToken, NativeContract};
    use neo_primitives::UInt160;
    use neo_storage::persistence::DataCache;
    use num_bigint::BigInt;
    use std::sync::Arc;

    #[test]
    fn test_gas_token_constants() {
        let gas = GasToken::new();

        // Verify constants match C# implementation
        assert_eq!(gas.symbol(), "GAS");
        assert_eq!(gas.decimals(), 8);
        assert_eq!(gas.id(), -6);

        // Verify hash matches C# GasToken hash
        let expected_hash = "0xd2a4cff31913016155e38e474a2c06d08be276cf";
        assert_eq!(gas.hash().to_string(), expected_hash);
    }

    #[test]
    fn test_initial_supply() {
        let snapshot = Arc::new(DataCache::new(false));
        let gas = GasToken::new();

        // Initial supply should be zero (minted during genesis)
        let supply = gas.total_supply_snapshot(snapshot.as_ref());
        assert_eq!(supply, BigInt::from(0));
    }

    #[test]
    fn test_balance_of_zero_account() {
        let snapshot = Arc::new(DataCache::new(false));
        let gas = GasToken::new();
        let account = UInt160::from([0u8; 20]);

        let balance = gas.balance_of_snapshot(snapshot.as_ref(), &account);
        assert_eq!(balance, BigInt::from(0));
    }
}
