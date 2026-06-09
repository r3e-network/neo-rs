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
use neo_serialization::BinarySerializer;
use neo_storage::persistence::DataCache;
use neo_storage::StorageKey;
use neo_vm::StackItem;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;

use crate::hashes::NOTARY_HASH;

/// Lazily-initialised script-hash handle for the Notary contract.
pub static NOTARY_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *NOTARY_HASH);

/// Storage prefix for the max-NotValidBefore-delta setting (C#
/// `Notary.Prefix_MaxNotValidBeforeDelta`).
const PREFIX_MAX_NOT_VALID_BEFORE_DELTA: u8 = 10;
/// C# `Notary.DefaultMaxNotValidBeforeDelta`.
const DEFAULT_MAX_NOT_VALID_BEFORE_DELTA: i64 = 140;
/// C# `Notary.Prefix_Deposit` — per-account deposit (`Struct[Amount, Till]`).
const PREFIX_DEPOSIT: u8 = 1;

/// Reads field `index` of the C# `Deposit` struct (`[Amount, Till]`) stored under
/// `Prefix_Deposit ++ account`, returning 0 when the account has no deposit.
/// `balanceOf` reads `Amount` (index 0); `expirationOf` reads `Till` (index 1).
fn read_deposit_field(snapshot: &DataCache, account: &UInt160, index: usize) -> CoreResult<BigInt> {
    let mut key_bytes = vec![PREFIX_DEPOSIT];
    key_bytes.extend_from_slice(&account.to_bytes());
    let Some(item) = snapshot.get(&StorageKey::new(Notary::ID, key_bytes)) else {
        return Ok(BigInt::from(0));
    };
    let state =
        BinarySerializer::deserialize(&item.value_bytes(), &ExecutionEngineLimits::default(), None)
            .map_err(|e| CoreError::deserialization(format!("Notary deposit: {e}")))?;
    let StackItem::Struct(fields) = state else {
        return Err(CoreError::invalid_data("Notary deposit is not a struct"));
    };
    let items = fields.items();
    let field = items
        .get(index)
        .ok_or_else(|| CoreError::invalid_data("Notary deposit field is missing"))?;
    field
        .as_int()
        .map_err(|e| CoreError::invalid_data(format!("Notary deposit field: {e}")))
}

/// Parses the leading `Hash160` account argument for the deposit reads.
fn parse_account(args: &[Vec<u8>], method: &str) -> CoreResult<UInt160> {
    let bytes = args
        .first()
        .ok_or_else(|| CoreError::invalid_operation(format!("Notary::{method} requires an account")))?;
    UInt160::from_bytes(bytes)
        .map_err(|e| CoreError::invalid_operation(format!("Notary::{method}: bad account: {e}")))
}

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
    let read_states = CallFlags::READ_STATES.bits();
    let int = ContractParameterType::Integer;
    vec![
        NativeMethod::new(
            "getMaxNotValidBeforeDelta".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            int,
        ),
        // Deposit reads: balanceOf -> Amount, expirationOf -> Till.
        NativeMethod::new(
            "balanceOf".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            int,
        ),
        NativeMethod::new(
            "expirationOf".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            int,
        ),
    ]
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
        args: &[Vec<u8>],
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
            "balanceOf" => {
                let account = parse_account(args, "balanceOf")?;
                Ok(read_deposit_field(&snapshot, &account, 0)?.to_signed_bytes_le())
            }
            "expirationOf" => {
                let account = parse_account(args, "expirationOf")?;
                Ok(read_deposit_field(&snapshot, &account, 1)?.to_signed_bytes_le())
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
        assert_eq!(names, ["getMaxNotValidBeforeDelta", "balanceOf", "expirationOf"]);
        for name in ["balanceOf", "expirationOf"] {
            let m = c.methods().iter().find(|m| m.name == name).unwrap();
            assert_eq!(m.parameters, vec![ContractParameterType::Hash160]);
            assert_eq!(m.return_type, ContractParameterType::Integer);
        }
    }

    #[test]
    fn deposit_reads_amount_and_till_or_zero() {
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[3u8; 20]).unwrap();

        // Absent deposit -> both reads are 0.
        assert_eq!(read_deposit_field(&cache, &account, 0).unwrap(), BigInt::from(0));
        assert_eq!(read_deposit_field(&cache, &account, 1).unwrap(), BigInt::from(0));

        // Store a Deposit struct [Amount=1000, Till=42] and read each field.
        let deposit = StackItem::from_struct(vec![
            StackItem::from_int(1000),
            StackItem::from_int(42),
        ]);
        let bytes =
            BinarySerializer::serialize(&deposit, &ExecutionEngineLimits::default()).unwrap();
        let mut key_bytes = vec![PREFIX_DEPOSIT];
        key_bytes.extend_from_slice(&account.to_bytes());
        cache.add(StorageKey::new(Notary::ID, key_bytes), StorageItem::from_bytes(bytes));

        assert_eq!(read_deposit_field(&cache, &account, 0).unwrap(), BigInt::from(1000)); // Amount
        assert_eq!(read_deposit_field(&cache, &account, 1).unwrap(), BigInt::from(42)); // Till
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
