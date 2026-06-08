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

/// Treasury contract hash (Neo N3 reserved, not yet activated).
///
/// The Treasury contract is part of the canonical Neo native-contract
/// set but is gated behind a hardfork that has not been deployed yet.
/// The hash is computed identically to the other native contracts.
pub static TREASURY_HASH: LazyLock<UInt160> = LazyLock::new(|| {
    UInt160::parse("0xed076e9c9d446e842a6a845c8a4a3a7c8d9ac14f")
        .expect("TREASURY_HASH is a valid hex string")
});
