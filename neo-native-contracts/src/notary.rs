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
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::hashes::NOTARY_HASH;
use crate::LedgerContract;

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

/// The deposit storage key `(Notary.ID, [Prefix_Deposit, account])`.
fn deposit_key(account: &UInt160) -> StorageKey {
    let mut key_bytes = vec![PREFIX_DEPOSIT];
    key_bytes.extend_from_slice(&account.to_bytes());
    StorageKey::new(Notary::ID, key_bytes)
}

/// Reads the full `Deposit` `(Amount, Till)` for `account`, or `None` when the
/// account has no deposit (C# `GetDepositFor` returning null).
fn read_deposit(snapshot: &DataCache, account: &UInt160) -> CoreResult<Option<(BigInt, u32)>> {
    let Some(item) = snapshot.get(&deposit_key(account)) else {
        return Ok(None);
    };
    let state =
        BinarySerializer::deserialize(&item.value_bytes(), &ExecutionEngineLimits::default(), None)
            .map_err(|e| CoreError::deserialization(format!("Notary deposit: {e}")))?;
    let StackItem::Struct(fields) = state else {
        return Err(CoreError::invalid_data("Notary deposit is not a struct"));
    };
    let items = fields.items();
    let amount = items
        .first()
        .ok_or_else(|| CoreError::invalid_data("Notary deposit Amount missing"))?
        .as_int()
        .map_err(|e| CoreError::invalid_data(format!("Notary deposit Amount: {e}")))?;
    let till = items
        .get(1)
        .ok_or_else(|| CoreError::invalid_data("Notary deposit Till missing"))?
        .as_int()
        .map_err(|e| CoreError::invalid_data(format!("Notary deposit Till: {e}")))?
        .to_u32()
        .ok_or_else(|| CoreError::invalid_data("Notary deposit Till out of range"))?;
    Ok(Some((amount, till)))
}

/// Writes the `Deposit` `(Amount, Till)` struct for `account` (C# `PutDepositFor`):
/// the BinarySerialized `Struct[Amount, Till]`.
fn write_deposit(snapshot: &DataCache, account: &UInt160, amount: &BigInt, till: u32) -> CoreResult<()> {
    let item = StackItem::from_struct(vec![
        StackItem::from_int(amount.clone()),
        StackItem::from_int(till),
    ]);
    let bytes = BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())
        .map_err(|e| CoreError::serialization(format!("Notary deposit serialize: {e}")))?;
    snapshot.update(deposit_key(account), StorageItem::from_bytes(bytes));
    Ok(())
}

/// Pure decision core of C# `LockDepositUntil` (after the witness check): returns
/// `Some((amount, till))` to write, or `None` to return `false`. The new `till`
/// must be at least `current_index + 2` (so the deposit outlives the next block)
/// and at least the deposit's existing `Till` (locks cannot be shortened), and a
/// deposit must already exist. `wrapping_add` matches C#'s unchecked `uint` math.
fn lock_deposit_decision(
    current_index: u32,
    deposit: Option<(BigInt, u32)>,
    till: u32,
) -> Option<(BigInt, u32)> {
    if till < current_index.wrapping_add(2) {
        return None;
    }
    let (amount, existing_till) = deposit?;
    if till < existing_till {
        return None;
    }
    Some((amount, till))
}

/// C# `SetMaxNotValidBeforeDelta` storage effect: overwrite
/// `Prefix_MaxNotValidBeforeDelta` (`GetAndChange(...).Set(value)`). The key is
/// genesis-initialised (`OnPersist` Add), so `update` (= C# GetAndChange) is the
/// correct primitive.
fn put_max_not_valid_before_delta(snapshot: &DataCache, value: i64) {
    snapshot.update(
        StorageKey::new(Notary::ID, vec![PREFIX_MAX_NOT_VALID_BEFORE_DELTA]),
        StorageItem::from_bytes(BigInt::from(value).to_signed_bytes_le()),
    );
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
        // Committee-gated setter: not safe, States, Integer -> Void.
        NativeMethod::new(
            "setMaxNotValidBeforeDelta".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![int],
            ContractParameterType::Void,
        ),
        // lockDepositUntil(account, till) -> bool: account-witnessed, States.
        NativeMethod::new(
            "lockDepositUntil".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Hash160, int],
            ContractParameterType::Boolean,
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
            "lockDepositUntil" => {
                // C#: CheckWitnessInternal(account) (false return on no witness),
                // then till >= currentIndex+2, an existing deposit, and till not
                // shortening it; on success update Deposit.Till and write back.
                let account = parse_account(args, "lockDepositUntil")?;
                let till = args
                    .get(1)
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_u32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation("Notary::lockDepositUntil requires a uint till")
                    })?;
                // CheckWitnessInternal: a missing witness returns false (not a fault).
                let witnessed = engine.check_witness(&account).map_err(|e| {
                    CoreError::invalid_operation(format!("lockDepositUntil witness: {e}"))
                })?;
                if !witnessed {
                    return Ok(vec![0]);
                }
                let current = LedgerContract::new().current_index(&snapshot)?;
                let deposit = read_deposit(&snapshot, &account)?;
                match lock_deposit_decision(current, deposit, till) {
                    Some((amount, new_till)) => {
                        write_deposit(&engine.snapshot_cache(), &account, &amount, new_till)?;
                        Ok(vec![1])
                    }
                    None => Ok(vec![0]),
                }
            }
            "setMaxNotValidBeforeDelta" => {
                // C# param is `uint value`: decode as u32 (out-of-range faults like
                // the C# uint parameter binding).
                let value = args
                    .first()
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_u32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "Notary::setMaxNotValidBeforeDelta requires a uint value",
                        )
                    })?;
                // C# bound: value must be ≤ GetMaxValidUntilBlockIncrement/2 and ≥
                // ProtocolSettings.Default.ValidatorsCount. The default settings'
                // ValidatorsCount is 0, so `value < 0` can never hold for a uint —
                // the lower bound is a faithful no-op and only the upper bound
                // (hardfork-aware MaxValidUntilBlockIncrement / 2) can fault.
                let upper = crate::policy_contract::read_max_valid_until_block_increment(engine)? / 2;
                if i64::from(value) > upper {
                    return Err(CoreError::invalid_operation(format!(
                        "MaxNotValidBeforeDelta cannot be more than {upper} or less than 0"
                    )));
                }
                let authorized = engine.check_committee_witness().map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "setMaxNotValidBeforeDelta committee check: {e}"
                    ))
                })?;
                if !authorized {
                    return Err(CoreError::invalid_operation(
                        "setMaxNotValidBeforeDelta requires committee authorization",
                    ));
                }
                put_max_not_valid_before_delta(&engine.snapshot_cache(), i64::from(value));
                Ok(Vec::new())
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
        assert_eq!(
            names,
            [
                "getMaxNotValidBeforeDelta",
                "balanceOf",
                "expirationOf",
                "setMaxNotValidBeforeDelta",
                "lockDepositUntil"
            ]
        );
        for name in ["balanceOf", "expirationOf"] {
            let m = c.methods().iter().find(|m| m.name == name).unwrap();
            assert_eq!(m.parameters, vec![ContractParameterType::Hash160]);
            assert_eq!(m.return_type, ContractParameterType::Integer);
        }
        // The committee-gated setter: not safe, States, Integer -> Void.
        let setter = c
            .methods()
            .iter()
            .find(|m| m.name == "setMaxNotValidBeforeDelta")
            .unwrap();
        assert!(!setter.safe);
        assert_eq!(setter.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(setter.parameters, vec![ContractParameterType::Integer]);
        assert_eq!(setter.return_type, ContractParameterType::Void);
        assert_eq!(setter.cpu_fee, 1 << 15);
        // lockDepositUntil: not safe, States, (Hash160, Integer) -> Boolean.
        let lock = c.methods().iter().find(|m| m.name == "lockDepositUntil").unwrap();
        assert!(!lock.safe);
        assert_eq!(lock.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(
            lock.parameters,
            vec![ContractParameterType::Hash160, ContractParameterType::Integer]
        );
        assert_eq!(lock.return_type, ContractParameterType::Boolean);
    }

    #[test]
    fn deposit_round_trips_and_lock_decision_matches_csharp() {
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[7u8; 20]).unwrap();

        // No deposit -> read_deposit None; lock decision -> None (false).
        assert!(read_deposit(&cache, &account).unwrap().is_none());
        assert!(lock_deposit_decision(100, None, 200).is_none());

        // Write a deposit (Amount=1000, Till=150) and read it back.
        write_deposit(&cache, &account, &BigInt::from(1000), 150).unwrap();
        assert_eq!(
            read_deposit(&cache, &account).unwrap(),
            Some((BigInt::from(1000), 150))
        );

        let deposit = read_deposit(&cache, &account).unwrap();
        // till below current+2 -> None.
        assert!(lock_deposit_decision(199, deposit.clone(), 200).is_none());
        // till below existing Till (150) -> None (can't shorten).
        assert!(lock_deposit_decision(100, deposit.clone(), 149).is_none());
        // Valid extension keeps Amount, updates Till.
        assert_eq!(
            lock_deposit_decision(100, deposit, 300),
            Some((BigInt::from(1000), 300))
        );

        // The lock write preserves Amount and updates Till.
        write_deposit(&cache, &account, &BigInt::from(1000), 300).unwrap();
        assert_eq!(
            read_deposit(&cache, &account).unwrap(),
            Some((BigInt::from(1000), 300))
        );
    }

    #[test]
    fn set_max_not_valid_before_delta_write_round_trips() {
        // The setter's storage effect (overwrite Prefix_MaxNotValidBeforeDelta) is
        // observed by the getMaxNotValidBeforeDelta reader, matching C#
        // GetAndChange(...).Set(value).
        let cache = DataCache::new(false);
        assert_eq!(
            crate::read_storage_int(
                &cache,
                Notary::ID,
                PREFIX_MAX_NOT_VALID_BEFORE_DELTA,
                DEFAULT_MAX_NOT_VALID_BEFORE_DELTA
            )
            .unwrap(),
            DEFAULT_MAX_NOT_VALID_BEFORE_DELTA
        );
        put_max_not_valid_before_delta(&cache, 250);
        assert_eq!(
            crate::read_storage_int(
                &cache,
                Notary::ID,
                PREFIX_MAX_NOT_VALID_BEFORE_DELTA,
                DEFAULT_MAX_NOT_VALID_BEFORE_DELTA
            )
            .unwrap(),
            250
        );
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
