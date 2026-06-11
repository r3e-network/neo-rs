//! Well-known native contract script hashes.
//!
//! The 11 standard native contracts have hard-coded script hashes that
//! are the same across the entire Neo network (computed from the
//! native contract's manifest using `Helper::get_contract_hash(zero,
//! 0, name)`).
//!
//! Values below are taken from the Neo N3 MainNet/TestNet reference
//! implementation and verified by the
//! `print_canonical_native_hashes` integration test. They are exposed
//! as [`std::sync::LazyLock`] values (rather than `const`) because
//! `UInt160::parse` is not `const`.

use neo_primitives::UInt160;
use std::sync::LazyLock;

/// ContractManagement contract hash.
pub static CONTRACT_MANAGEMENT_HASH: LazyLock<UInt160> = LazyLock::new(|| {
    UInt160::parse("0xfffdc93764dbaddd97c48f252a53ea4643faa3fd")
        .expect("CONTRACT_MANAGEMENT_HASH is a valid hex string")
});

/// StdLib contract hash.
pub static STDLIB_HASH: LazyLock<UInt160> = LazyLock::new(|| {
    UInt160::parse("0xacce6fd80d44e1796aa0c2c625e9e4e0ce39efc0")
        .expect("STDLIB_HASH is a valid hex string")
});

/// CryptoLib contract hash (BLS12-381).
pub static CRYPTO_LIB_HASH: LazyLock<UInt160> = LazyLock::new(|| {
    UInt160::parse("0x726cb6e0cd8628a1350a611384688911ab75f51b")
        .expect("CRYPTO_LIB_HASH is a valid hex string")
});

/// Ledger contract hash.
pub static LEDGER_CONTRACT_HASH: LazyLock<UInt160> = LazyLock::new(|| {
    UInt160::parse("0xda65b600f7124ce6c79950c1772a36403104f2be")
        .expect("LEDGER_CONTRACT_HASH is a valid hex string")
});

/// NEO token contract hash.
pub static NEO_TOKEN_HASH: LazyLock<UInt160> = LazyLock::new(|| {
    UInt160::parse("0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5")
        .expect("NEO_TOKEN_HASH is a valid hex string")
});

/// GAS token contract hash.
pub static GAS_TOKEN_HASH: LazyLock<UInt160> = LazyLock::new(|| {
    UInt160::parse("0xd2a4cff31913016155e38e474a2c06d08be276cf")
        .expect("GAS_TOKEN_HASH is a valid hex string")
});

/// Policy contract hash.
pub static POLICY_CONTRACT_HASH: LazyLock<UInt160> = LazyLock::new(|| {
    UInt160::parse("0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b")
        .expect("POLICY_CONTRACT_HASH is a valid hex string")
});

/// RoleManagement contract hash.
pub static ROLE_MANAGEMENT_HASH: LazyLock<UInt160> = LazyLock::new(|| {
    UInt160::parse("0x49cf4e5378ffcd4dec034fd98a174c5491e395e2")
        .expect("ROLE_MANAGEMENT_HASH is a valid hex string")
});

/// Oracle contract hash.
pub static ORACLE_CONTRACT_HASH: LazyLock<UInt160> = LazyLock::new(|| {
    UInt160::parse("0xfe924b7cfe89ddd271abaf7210a80a7e11178758")
        .expect("ORACLE_CONTRACT_HASH is a valid hex string")
});

/// Notary contract hash.
pub static NOTARY_HASH: LazyLock<UInt160> = LazyLock::new(|| {
    UInt160::parse("0xc1e14f19c3e60d0b9244d06dd7ba9b113135ec3b")
        .expect("NOTARY_HASH is a valid hex string")
});

/// Treasury contract hash (activated by the `HF_Faun` hardfork).
///
/// Computed identically to the other native contracts as
/// `get_contract_hash(UInt160::ZERO, 0, "Treasury")`. Verified against the
/// C# Neo v3.9.1 reference (`UT_NativeContract.cs`, which pins
/// `"hash":"0x156326f25b1b5d839a4d326aeaa75383c9563ac1"` for Treasury).
pub static TREASURY_HASH: LazyLock<UInt160> = LazyLock::new(|| {
    UInt160::parse("0x156326f25b1b5d839a4d326aeaa75383c9563ac1")
        .expect("TREASURY_HASH is a valid hex string")
});

#[cfg(test)]
mod parity_tests {
    use super::*;
    use neo_execution::Helper;
    use neo_primitives::UInt160;

    /// Verifies our consensus-critical contract-hash derivation reproduces every
    /// canonical Neo N3 native-contract hash.
    ///
    /// In C# Neo v3.9.1 each native contract sets
    /// `Hash = Helper.GetContractHash(UInt160.Zero, 0, Name)`, where
    /// `Name => GetType().Name` (the class name). `GetContractHash` emits
    /// `ABORT, push(sender), push(nefCheckSum), push(name)` and takes the script
    /// hash (`sha256` then `ripemd160`). If our `ScriptBuilder` opcode/push
    /// encoding or script-hash derivation diverged from C#, the derived values
    /// would not match the well-known MainNet hashes pinned above — so this is a
    /// genuine cross-implementation parity check, not a self-referential one.
    #[test]
    fn native_contract_hashes_match_csharp_derivation() {
        let zero = UInt160::default();
        let cases: [(&str, UInt160); 11] = [
            ("ContractManagement", *CONTRACT_MANAGEMENT_HASH),
            ("StdLib", *STDLIB_HASH),
            ("CryptoLib", *CRYPTO_LIB_HASH),
            ("LedgerContract", *LEDGER_CONTRACT_HASH),
            ("NeoToken", *NEO_TOKEN_HASH),
            ("GasToken", *GAS_TOKEN_HASH),
            ("PolicyContract", *POLICY_CONTRACT_HASH),
            ("RoleManagement", *ROLE_MANAGEMENT_HASH),
            ("OracleContract", *ORACLE_CONTRACT_HASH),
            ("Notary", *NOTARY_HASH),
            ("Treasury", *TREASURY_HASH),
        ];
        for (name, expected) in cases {
            let derived = Helper::get_contract_hash(&zero, 0, name);
            assert_eq!(
                derived, expected,
                "get_contract_hash(0, 0, {name:?}) diverged from the canonical Neo N3 hash"
            );
        }
    }

    /// All native-contract hashes must be distinct (a collision would let two
    /// native contracts share storage and break consensus).
    #[test]
    fn native_contract_hashes_are_distinct() {
        let all = [
            *CONTRACT_MANAGEMENT_HASH,
            *STDLIB_HASH,
            *CRYPTO_LIB_HASH,
            *LEDGER_CONTRACT_HASH,
            *NEO_TOKEN_HASH,
            *GAS_TOKEN_HASH,
            *POLICY_CONTRACT_HASH,
            *ROLE_MANAGEMENT_HASH,
            *ORACLE_CONTRACT_HASH,
            *NOTARY_HASH,
            *TREASURY_HASH,
        ];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(
                    all[i], all[j],
                    "duplicate native contract hash at indices {i}/{j}"
                );
            }
        }
    }

    /// Native-contract ids must match the canonical Neo N3 values. The id is
    /// part of native storage-key derivation, so a wrong id silently diverges
    /// consensus. Pinned against the C# v3.9.1 reference (UT_NativeContract.cs).
    #[test]
    fn native_contract_ids_match_csharp() {
        use crate::{
            ContractManagement, CryptoLib, GasToken, LedgerContract, NeoToken, Notary,
            OracleContract, PolicyContract, RoleManagement, StdLib, Treasury,
        };
        assert_eq!(ContractManagement::ID, -1);
        assert_eq!(StdLib::ID, -2);
        assert_eq!(CryptoLib::ID, -3);
        assert_eq!(LedgerContract::ID, -4);
        assert_eq!(NeoToken::ID, -5);
        assert_eq!(GasToken::ID, -6);
        assert_eq!(PolicyContract::ID, -7);
        assert_eq!(RoleManagement::ID, -8);
        assert_eq!(OracleContract::ID, -9);
        assert_eq!(Notary::ID, -10);
        assert_eq!(Treasury::ID, -11);
    }
}
