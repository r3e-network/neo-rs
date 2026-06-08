//! Notary native contract (id -10).
//!
//! Implements the read-side `getMaxNotValidBeforeDelta` of the C#
//! `Neo.SmartContract.Native.Notary`. The stateful surface (deposits, `verify`,
//! `onNEP17Payment`, `withdraw`, ...) is the next increment on the
//! storage-backed pattern.

use std::any::Any;
use std::sync::LazyLock;

use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType, UInt160};
use num_bigint::BigInt;

use crate::hashes::NOTARY_HASH;

/// Lazily-initialised script-hash handle for the Notary contract.
pub static NOTARY_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *NOTARY_HASH);

/// Storage prefix for the max-NotValidBefore-delta setting (C#
/// `Notary.Prefix_MaxNotValidBeforeDelta`).
const PREFIX_MAX_NOT_VALID_BEFORE_DELTA: u8 = 10;
/// C# `Notary.DefaultMaxNotValidBeforeDelta`.
const DEFAULT_MAX_NOT_VALID_BEFORE_DELTA: i64 = 140;

/// The Notary native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct Notary;

impl Notary {
    /// Stable native contract id (matches C# `Notary`).
    pub const ID: i32 = -10;

    /// Construct a new `Notary` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the Notary script hash.
    pub fn script_hash() -> UInt160 {
        *NOTARY_HASH_REF
    }
}

static NOTARY_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    vec![NativeMethod::new(
        "getMaxNotValidBeforeDelta".to_string(),
        1 << 15,
        true,
        CallFlags::READ_STATES.bits(),
        vec![],
        ContractParameterType::Integer,
    )]
});

impl NativeContract for Notary {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *NOTARY_HASH_REF
    }

    fn name(&self) -> &str {
        "Notary"
    }

    fn methods(&self) -> &[NativeMethod] {
        &NOTARY_METHODS
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        match method {
            "getMaxNotValidBeforeDelta" => {
                let delta = crate::read_storage_int(
                    &snapshot,
                    Self::ID,
                    PREFIX_MAX_NOT_VALID_BEFORE_DELTA,
                    DEFAULT_MAX_NOT_VALID_BEFORE_DELTA,
                )?;
                Ok(BigInt::from(delta).to_signed_bytes_le())
            }
            other => Err(CoreError::invalid_operation(format!(
                "Notary method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::persistence::DataCache;
    use neo_storage::{StorageItem, StorageKey};

    #[test]
    fn native_contract_surface() {
        let c = Notary::new();
        assert_eq!(NativeContract::id(&c), -10);
        assert_eq!(NativeContract::name(&c), "Notary");
        assert_eq!(NativeContract::hash(&c), *NOTARY_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["getMaxNotValidBeforeDelta"]);
    }

    #[test]
    fn max_not_valid_before_delta_reads_storage_with_default() {
        let cache = DataCache::new(false);
        assert_eq!(
            crate::read_storage_int(&cache, Notary::ID, PREFIX_MAX_NOT_VALID_BEFORE_DELTA, 140)
                .unwrap(),
            140
        );
        let key = StorageKey::new(Notary::ID, vec![PREFIX_MAX_NOT_VALID_BEFORE_DELTA]);
        cache.add(key, StorageItem::from_bytes(BigInt::from(200).to_signed_bytes_le()));
        assert_eq!(
            crate::read_storage_int(&cache, Notary::ID, PREFIX_MAX_NOT_VALID_BEFORE_DELTA, 140)
                .unwrap(),
            200
        );
    }
}
