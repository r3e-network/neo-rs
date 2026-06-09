//! PolicyContract native contract stub.

use crate::hashes::POLICY_CONTRACT_HASH;
use neo_config::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType, UInt160};
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::any::Any;
use std::sync::LazyLock;

/// C# `PolicyContract.Prefix_FeePerByte` storage prefix.
const PREFIX_FEE_PER_BYTE: u8 = 10;
/// C# `PolicyContract.Prefix_StoragePrice` storage prefix.
const PREFIX_STORAGE_PRICE: u8 = 19;
/// C# `PolicyContract.DefaultStoragePrice`.
const DEFAULT_STORAGE_PRICE: i64 = 100_000;
/// C# `PolicyContract.Prefix_BlockedAccount` storage prefix.
const PREFIX_BLOCKED_ACCOUNT: u8 = 15;
/// C# `PolicyContract.Prefix_MillisecondsPerBlock` (HF_Echidna).
const PREFIX_MILLISECONDS_PER_BLOCK: u8 = 21;
/// C# `PolicyContract.Prefix_MaxValidUntilBlockIncrement` (HF_Echidna).
const PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT: u8 = 22;
/// C# `PolicyContract.Prefix_MaxTraceableBlocks` (HF_Echidna).
const PREFIX_MAX_TRACEABLE_BLOCKS: u8 = 23;

/// Lazily-initialised script-hash handle for the PolicyContract.
pub static POLICY_HASH: LazyLock<UInt160> = LazyLock::new(|| *POLICY_CONTRACT_HASH);

/// Default execution fee factor (matches C# `PolicyContract.DefaultExecFeeFactor`).
pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30;
/// Default fee per byte (matches C# `PolicyContract.DefaultFeePerByte`).
pub const DEFAULT_FEE_PER_BYTE: u32 = 1000;
/// Default max valid-until-block increment
/// (matches C# `PolicyContract.DefaultMaxValidUntilBlockIncrement`).
pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 5_760;

/// Static accessor for the PolicyContract native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct PolicyContract;

impl PolicyContract {
    /// Stable native contract id (-7 in C# Policy contract).
    pub const ID: i32 = -7;

    /// Default execution fee factor.
    pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30;
    /// Default fee per byte.
    pub const DEFAULT_FEE_PER_BYTE: u32 = 1000;
    /// Default max valid-until-block increment.
    pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 5_760;

    /// Construct a new `PolicyContract` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the Policy native contract.
    pub fn hash(&self) -> UInt160 {
        *POLICY_HASH
    }

    /// Returns the script hash of the Policy native contract (static).
    pub fn script_hash() -> UInt160 {
        *POLICY_HASH
    }

    /// Stub: returns the max valid-until-block increment from the
    /// snapshot, or `Ok(default)` if not configured.
    pub fn get_max_valid_until_block_increment_snapshot(
        &self,
        _snapshot: &neo_storage::persistence::DataCache,
        _settings: &neo_config::ProtocolSettings,
    ) -> neo_error::CoreResult<u32> {
        Ok(DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT)
    }

    /// Stub: returns the execution fee factor from the snapshot, or
    /// `Ok(DEFAULT_EXEC_FEE_FACTOR)` if not configured.
    pub fn get_exec_fee_factor_snapshot(
        &self,
        _snapshot: &neo_storage::persistence::DataCache,
        _settings: &neo_config::ProtocolSettings,
        _height: u32,
    ) -> neo_error::CoreResult<u32> {
        Ok(DEFAULT_EXEC_FEE_FACTOR)
    }

    /// Stub: returns the fee-per-byte from the snapshot, or
    /// `Ok(DEFAULT_FEE_PER_BYTE)` if not configured.
    pub fn get_fee_per_byte_snapshot(
        &self,
        _snapshot: &neo_storage::persistence::DataCache,
    ) -> neo_error::CoreResult<u32> {
        Ok(DEFAULT_FEE_PER_BYTE)
    }
}

/// C# `GetFeePerByte` = `(long)(BigInteger)snapshot[_feePerByte]`.
fn fee_per_byte(snapshot: &DataCache) -> CoreResult<i64> {
    crate::read_storage_int(
        snapshot,
        PolicyContract::ID,
        PREFIX_FEE_PER_BYTE,
        i64::from(DEFAULT_FEE_PER_BYTE),
    )
}

/// C# `GetStoragePrice` = `(uint)(BigInteger)snapshot[_storagePrice]`.
fn storage_price(snapshot: &DataCache) -> CoreResult<i64> {
    crate::read_storage_int(snapshot, PolicyContract::ID, PREFIX_STORAGE_PRICE, DEFAULT_STORAGE_PRICE)
}

/// C# upper bound on fee-per-byte: 1 GAS in datoshi (`SetFeePerByte` rejects
/// anything outside `[0, 100000000]`).
const MAX_FEE_PER_BYTE: i64 = 100_000_000;

/// C# `SetFeePerByte` range guard: the value must be in `[0, MAX_FEE_PER_BYTE]`.
fn validate_fee_per_byte(value: i64) -> CoreResult<()> {
    if !(0..=MAX_FEE_PER_BYTE).contains(&value) {
        return Err(CoreError::invalid_operation(format!(
            "FeePerByte must be between [0, {MAX_FEE_PER_BYTE}], got {value}"
        )));
    }
    Ok(())
}

/// Writes the fee-per-byte to `Prefix_FeePerByte` as a `BigInteger`, mirroring
/// C# `GetAndChange(_feePerByte).Set(value)` (overwrite-as-Changed semantics).
fn put_fee_per_byte(snapshot: &DataCache, value: i64) {
    let key = StorageKey::new(PolicyContract::ID, vec![PREFIX_FEE_PER_BYTE]);
    snapshot.update(key, StorageItem::from_bytes(BigInt::from(value).to_signed_bytes_le()));
}

/// C# upper bound on storage price: `PolicyContract.MaxStoragePrice`.
const MAX_STORAGE_PRICE: i64 = 10_000_000;

/// C# `SetStoragePrice` range guard: the value must be in `[1, MAX_STORAGE_PRICE]`
/// (C# rejects `value == 0 || value > MaxStoragePrice`).
fn validate_storage_price(value: i64) -> CoreResult<()> {
    if !(1..=MAX_STORAGE_PRICE).contains(&value) {
        return Err(CoreError::invalid_operation(format!(
            "StoragePrice must be between [1, {MAX_STORAGE_PRICE}], got {value}"
        )));
    }
    Ok(())
}

/// Writes the storage price to `Prefix_StoragePrice` as a `BigInteger`
/// (C# `GetAndChange(_storagePrice).Set(value)`).
fn put_storage_price(snapshot: &DataCache, value: i64) {
    let key = StorageKey::new(PolicyContract::ID, vec![PREFIX_STORAGE_PRICE]);
    snapshot.update(key, StorageItem::from_bytes(BigInt::from(value).to_signed_bytes_le()));
}

/// C# `NativeContract.AssertCommittee`: returns an error unless the committee
/// multisig address witnessed this call. Shared by all committee-gated setters.
fn assert_committee(engine: &ApplicationEngine, method: &str) -> CoreResult<()> {
    let authorized = engine
        .check_committee_witness()
        .map_err(|e| CoreError::invalid_operation(format!("{method} committee check: {e}")))?;
    if !authorized {
        return Err(CoreError::invalid_operation(format!(
            "{method} requires committee authorization"
        )));
    }
    Ok(())
}

/// The blocked-account storage key `(PolicyContract.ID, [Prefix_BlockedAccount,
/// account])`, shared by `isBlocked` / `blockAccount` / `unblockAccount`.
fn blocked_account_key(account: &UInt160) -> StorageKey {
    let mut key_bytes = vec![PREFIX_BLOCKED_ACCOUNT];
    key_bytes.extend_from_slice(&account.to_bytes());
    StorageKey::new(PolicyContract::ID, key_bytes)
}

/// Parses a single integer argument into an `i64` for a setter, faulting when
/// absent or out of `i64` range (C# marshals the Integer arg to `long`/`uint`).
fn setter_int_arg(args: &[Vec<u8>], method: &str) -> CoreResult<i64> {
    args.first()
        .map(|b| BigInt::from_signed_bytes_le(b))
        .ok_or_else(|| CoreError::invalid_operation(format!("{method} requires a value")))?
        .to_i64()
        .ok_or_else(|| CoreError::invalid_operation(format!("{method}: value out of range")))
}

/// C# `PolicyContract.MaxMillisecondsPerBlock`.
const MAX_MILLISECONDS_PER_BLOCK: i64 = 30_000;

/// C# `SetMillisecondsPerBlock` range guard: `[1, MaxMillisecondsPerBlock]`.
fn validate_milliseconds_per_block(value: i64) -> CoreResult<()> {
    if !(1..=MAX_MILLISECONDS_PER_BLOCK).contains(&value) {
        return Err(CoreError::invalid_operation(format!(
            "MillisecondsPerBlock must be between [1, {MAX_MILLISECONDS_PER_BLOCK}], got {value}"
        )));
    }
    Ok(())
}

/// C# `GetMillisecondsPerBlock`: the stored `Prefix_MillisecondsPerBlock`, or the
/// `ProtocolSettings` value written at HF_Echidna activation. Shared by the getter
/// and the setter (which reads the old value for its change event).
fn read_milliseconds_per_block(engine: &ApplicationEngine) -> CoreResult<i64> {
    let default = i64::from(engine.protocol_settings().milliseconds_per_block);
    let snapshot = engine.snapshot_cache();
    crate::read_storage_int(&snapshot, PolicyContract::ID, PREFIX_MILLISECONDS_PER_BLOCK, default)
}

/// Writes the milliseconds-per-block to `Prefix_MillisecondsPerBlock`
/// (C# `GetAndChange(_millisecondsPerBlock).Set(value)`).
fn put_milliseconds_per_block(snapshot: &DataCache, value: i64) {
    let key = StorageKey::new(PolicyContract::ID, vec![PREFIX_MILLISECONDS_PER_BLOCK]);
    snapshot.update(key, StorageItem::from_bytes(BigInt::from(value).to_signed_bytes_le()));
}

/// C# `PolicyContract.MaxMaxValidUntilBlockIncrement`.
const MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT: i64 = 86_400;
/// C# `PolicyContract.MaxMaxTraceableBlocks`.
const MAX_MAX_TRACEABLE_BLOCKS: i64 = 2_102_400;

/// C# `SetMaxValidUntilBlockIncrement` range guard: `[1, 86400]`.
fn validate_max_valid_until_block_increment(value: i64) -> CoreResult<()> {
    if !(1..=MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT).contains(&value) {
        return Err(CoreError::invalid_operation(format!(
            "MaxValidUntilBlockIncrement must be between [1, {MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT}], got {value}"
        )));
    }
    Ok(())
}

/// C# `SetMaxTraceableBlocks` range guard: `[1, 2102400]`.
fn validate_max_traceable_blocks(value: i64) -> CoreResult<()> {
    if !(1..=MAX_MAX_TRACEABLE_BLOCKS).contains(&value) {
        return Err(CoreError::invalid_operation(format!(
            "MaxTraceableBlocks must be between [1, {MAX_MAX_TRACEABLE_BLOCKS}], got {value}"
        )));
    }
    Ok(())
}

/// C# `GetMaxValidUntilBlockIncrement`: stored `Prefix_MaxValidUntilBlockIncrement`,
/// defaulting to the `ProtocolSettings` value (written at HF_Echidna activation).
///
/// Exposed `pub(crate)` so other native contracts (e.g. `Notary`) can reuse the
/// hardfork-aware source, matching the C# extension
/// `IReadOnlyStore.GetMaxValidUntilBlockIncrement(ProtocolSettings)` (pre-Echidna
/// the protocol setting; from Echidna the Policy storage value).
pub(crate) fn read_max_valid_until_block_increment(engine: &ApplicationEngine) -> CoreResult<i64> {
    let default = i64::from(engine.protocol_settings().max_valid_until_block_increment);
    let snapshot = engine.snapshot_cache();
    crate::read_storage_int(
        &snapshot,
        PolicyContract::ID,
        PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT,
        default,
    )
}

/// Writes `Prefix_MaxValidUntilBlockIncrement` (C# `GetAndChange(...).Set(value)`).
fn put_max_valid_until_block_increment(snapshot: &DataCache, value: i64) {
    let key = StorageKey::new(PolicyContract::ID, vec![PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT]);
    snapshot.update(key, StorageItem::from_bytes(BigInt::from(value).to_signed_bytes_le()));
}

/// Writes `Prefix_MaxTraceableBlocks` (C# `GetAndChange(_maxTraceableBlocks).Set(value)`).
fn put_max_traceable_blocks(snapshot: &DataCache, value: i64) {
    let key = StorageKey::new(PolicyContract::ID, vec![PREFIX_MAX_TRACEABLE_BLOCKS]);
    snapshot.update(key, StorageItem::from_bytes(BigInt::from(value).to_signed_bytes_le()));
}

/// Returns the effective `MaxTraceableBlocks` for traceability checks, mirroring
/// the source selection in C# `LedgerContract.IsTraceableBlock`: before
/// `HF_Echidna` it is the static `ProtocolSettings.MaxTraceableBlocks`; from
/// `HF_Echidna` onward it is the committee-adjustable Policy value (storage
/// prefix 23), written at activation to `ProtocolSettings.MaxTraceableBlocks`.
///
/// Lives in PolicyContract because C# reads it via `Policy.GetMaxTraceableBlocks`;
/// keeping the prefix/default here is the single source of truth shared with the
/// `getMaxTraceableBlocks` getter.
pub(crate) fn max_traceable_blocks(engine: &ApplicationEngine) -> CoreResult<u32> {
    let default = engine.protocol_settings().max_traceable_blocks;
    if !engine.is_hardfork_enabled(Hardfork::HfEchidna) {
        return Ok(default);
    }
    let snapshot = engine.snapshot_cache();
    let value = crate::read_storage_int(
        &snapshot,
        PolicyContract::ID,
        PREFIX_MAX_TRACEABLE_BLOCKS,
        i64::from(default),
    )?;
    u32::try_from(value)
        .map_err(|_| CoreError::invalid_operation("MaxTraceableBlocks out of u32 range"))
}

static POLICY_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        NativeMethod::new(
            "getFeePerByte".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
        NativeMethod::new(
            "getStoragePrice".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
        // Committee-gated setters: not safe, require write (States) call flags.
        NativeMethod::new(
            "setFeePerByte".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        ),
        NativeMethod::new(
            "setStoragePrice".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        ),
        // HF_Echidna setter that emits a change notification (States|AllowNotify).
        NativeMethod::new(
            "setMillisecondsPerBlock".to_string(),
            1 << 15,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_active_in(Hardfork::HfEchidna),
        // HF_Echidna chain-parameter setters with cross-value invariants (States).
        NativeMethod::new(
            "setMaxValidUntilBlockIncrement".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_active_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "setMaxTraceableBlocks".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_active_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "isBlocked".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Boolean,
        ),
        // Committee-gated unblock writer (not safe, States, Boolean return).
        NativeMethod::new(
            "unblockAccount".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Hash160],
            ContractParameterType::Boolean,
        ),
        // HF_Echidna moved these chain parameters from ProtocolSettings into
        // PolicyContract storage; the getters default to the settings value.
        NativeMethod::new(
            "getMillisecondsPerBlock".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        )
        .with_active_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "getMaxValidUntilBlockIncrement".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        )
        .with_active_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "getMaxTraceableBlocks".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        )
        .with_active_in(Hardfork::HfEchidna),
    ]
});

impl NativeContract for PolicyContract {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *POLICY_HASH
    }

    fn name(&self) -> &str {
        "PolicyContract"
    }

    fn methods(&self) -> &[NativeMethod] {
        &POLICY_METHODS
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
            "getFeePerByte" => Ok(BigInt::from(fee_per_byte(&snapshot)?).to_signed_bytes_le()),
            "getStoragePrice" => Ok(BigInt::from(storage_price(&snapshot)?).to_signed_bytes_le()),
            "setFeePerByte" => {
                // C# order: validate range, then AssertCommittee, then write.
                let value = setter_int_arg(args, "setFeePerByte")?;
                validate_fee_per_byte(value)?;
                assert_committee(engine, "setFeePerByte")?;
                put_fee_per_byte(&engine.snapshot_cache(), value);
                Ok(Vec::new())
            }
            "setStoragePrice" => {
                let value = setter_int_arg(args, "setStoragePrice")?;
                validate_storage_price(value)?;
                assert_committee(engine, "setStoragePrice")?;
                put_storage_price(&engine.snapshot_cache(), value);
                Ok(Vec::new())
            }
            "isBlocked" => {
                let account_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation("PolicyContract::isBlocked requires an account")
                })?;
                let account = UInt160::from_bytes(account_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!("PolicyContract::isBlocked: bad account: {e}"))
                })?;
                // C# IsBlocked = snapshot.Contains(key(Prefix_BlockedAccount, account)).
                let blocked = snapshot.get(&blocked_account_key(&account)).is_some();
                Ok(vec![u8::from(blocked)])
            }
            "unblockAccount" => {
                // C#: AssertCommittee -> if not blocked return false ->
                // delete the entry -> return true.
                let account_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation("PolicyContract::unblockAccount requires an account")
                })?;
                let account = UInt160::from_bytes(account_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "PolicyContract::unblockAccount: bad account: {e}"
                    ))
                })?;
                assert_committee(engine, "unblockAccount")?;
                let key = blocked_account_key(&account);
                let snapshot = engine.snapshot_cache();
                let was_blocked = snapshot.get(&key).is_some();
                if was_blocked {
                    snapshot.delete(&key);
                }
                Ok(vec![u8::from(was_blocked)])
            }
            "getMillisecondsPerBlock" => {
                Ok(BigInt::from(read_milliseconds_per_block(engine)?).to_signed_bytes_le())
            }
            "setMillisecondsPerBlock" => {
                // C#: validate range -> AssertCommittee -> read old -> write ->
                // emit MillisecondsPerBlockChanged[oldValue, newValue].
                let value = setter_int_arg(args, "setMillisecondsPerBlock")?;
                validate_milliseconds_per_block(value)?;
                assert_committee(engine, "setMillisecondsPerBlock")?;
                let old = read_milliseconds_per_block(engine)?;
                put_milliseconds_per_block(&engine.snapshot_cache(), value);
                engine
                    .send_notification(
                        Self::script_hash(),
                        "MillisecondsPerBlockChanged".to_string(),
                        vec![StackItem::from_int(old), StackItem::from_int(value)],
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "setMillisecondsPerBlock notify: {e}"
                        ))
                    })?;
                Ok(Vec::new())
            }
            "setMaxValidUntilBlockIncrement" => {
                // C#: range [1, 86400] -> value < MaxTraceableBlocks -> committee.
                let value = setter_int_arg(args, "setMaxValidUntilBlockIncrement")?;
                validate_max_valid_until_block_increment(value)?;
                let mtb = max_traceable_blocks(engine)? as i64;
                if value >= mtb {
                    return Err(CoreError::invalid_operation(format!(
                        "MaxValidUntilBlockIncrement must be lower than MaxTraceableBlocks ({value} vs {mtb})"
                    )));
                }
                assert_committee(engine, "setMaxValidUntilBlockIncrement")?;
                put_max_valid_until_block_increment(&engine.snapshot_cache(), value);
                Ok(Vec::new())
            }
            "setMaxTraceableBlocks" => {
                // C#: range [1, 2102400] -> can only decrease -> value >
                // MaxValidUntilBlockIncrement -> committee.
                let value = setter_int_arg(args, "setMaxTraceableBlocks")?;
                validate_max_traceable_blocks(value)?;
                let old = max_traceable_blocks(engine)? as i64;
                if value > old {
                    return Err(CoreError::invalid_operation(format!(
                        "MaxTraceableBlocks can not be increased (old {old}, new {value})"
                    )));
                }
                let mvub = read_max_valid_until_block_increment(engine)?;
                if value <= mvub {
                    return Err(CoreError::invalid_operation(format!(
                        "MaxTraceableBlocks must be larger than MaxValidUntilBlockIncrement ({value} vs {mvub})"
                    )));
                }
                assert_committee(engine, "setMaxTraceableBlocks")?;
                put_max_traceable_blocks(&engine.snapshot_cache(), value);
                Ok(Vec::new())
            }
            "getMaxValidUntilBlockIncrement" => {
                Ok(BigInt::from(read_max_valid_until_block_increment(engine)?).to_signed_bytes_le())
            }
            "getMaxTraceableBlocks" => {
                Ok(BigInt::from(max_traceable_blocks(engine)? as i64).to_signed_bytes_le())
            }
            other => Err(CoreError::invalid_operation(format!(
                "PolicyContract method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::StorageItem;

    #[test]
    fn native_contract_surface() {
        let c = PolicyContract::new();
        assert_eq!(NativeContract::id(&c), -7);
        assert_eq!(NativeContract::name(&c), "PolicyContract");
        assert_eq!(NativeContract::hash(&c), *POLICY_CONTRACT_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "getFeePerByte",
                "getStoragePrice",
                "setFeePerByte",
                "setStoragePrice",
                "setMillisecondsPerBlock",
                "setMaxValidUntilBlockIncrement",
                "setMaxTraceableBlocks",
                "isBlocked",
                "unblockAccount",
                "getMillisecondsPerBlock",
                "getMaxValidUntilBlockIncrement",
                "getMaxTraceableBlocks"
            ]
        );
        // The Echidna-era chain-parameter getters are hardfork-gated.
        let mtb = c.methods().iter().find(|m| m.name == "getMaxTraceableBlocks").unwrap();
        assert_eq!(mtb.active_in, Some(Hardfork::HfEchidna));
        // unblockAccount is a non-safe, write-flagged (States), Boolean writer.
        let unblock = c.methods().iter().find(|m| m.name == "unblockAccount").unwrap();
        assert!(!unblock.safe);
        assert_eq!(unblock.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(unblock.parameters, vec![ContractParameterType::Hash160]);
        assert_eq!(unblock.return_type, ContractParameterType::Boolean);
        // The fee/price setters are non-safe, write-flagged (States), Void methods.
        for name in ["setFeePerByte", "setStoragePrice"] {
            let setter = c.methods().iter().find(|m| m.name == name).unwrap();
            assert!(!setter.safe, "{name} must not be safe");
            assert_eq!(setter.required_call_flags, CallFlags::STATES.bits());
            assert_eq!(setter.parameters, vec![ContractParameterType::Integer]);
            assert_eq!(setter.return_type, ContractParameterType::Void);
        }
        // The Echidna setter additionally emits a notification (States|AllowNotify).
        let ms = c.methods().iter().find(|m| m.name == "setMillisecondsPerBlock").unwrap();
        assert!(!ms.safe);
        assert_eq!(
            ms.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert_eq!(ms.return_type, ContractParameterType::Void);
        assert_eq!(ms.active_in, Some(Hardfork::HfEchidna));
        // The cross-validated Echidna setters are non-safe, States, Void, gated.
        for name in ["setMaxValidUntilBlockIncrement", "setMaxTraceableBlocks"] {
            let m = c.methods().iter().find(|m| m.name == name).unwrap();
            assert!(!m.safe, "{name} must not be safe");
            assert_eq!(m.required_call_flags, CallFlags::STATES.bits());
            assert_eq!(m.return_type, ContractParameterType::Void);
            assert_eq!(m.active_in, Some(Hardfork::HfEchidna));
        }
    }

    #[test]
    fn set_fee_per_byte_validation_bounds() {
        // C# SetFeePerByte accepts [0, 100000000] and rejects outside.
        assert!(validate_fee_per_byte(0).is_ok());
        assert!(validate_fee_per_byte(MAX_FEE_PER_BYTE).is_ok());
        assert!(validate_fee_per_byte(-1).is_err());
        assert!(validate_fee_per_byte(MAX_FEE_PER_BYTE + 1).is_err());
    }

    #[test]
    fn blocked_account_key_block_then_unblock_storage_effect() {
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[4u8; 20]).unwrap();
        let key = blocked_account_key(&account);
        // Not blocked initially.
        assert!(cache.get(&key).is_none());
        // Block (add) then unblock (delete) — the exact storage effect the
        // isBlocked / unblockAccount arms rely on.
        cache.add(key.clone(), StorageItem::from_bytes(vec![]));
        assert!(cache.get(&key).is_some());
        cache.delete(&key);
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn fee_per_byte_write_then_read_round_trips() {
        let cache = DataCache::new(false);
        // Writing via the setter's storage effect is observed by the getter,
        // exercising the GetAndChange (overwrite-as-Changed) semantics.
        put_fee_per_byte(&cache, 4242);
        assert_eq!(fee_per_byte(&cache).unwrap(), 4242);
        // Overwriting an existing value is read back as the new value.
        put_fee_per_byte(&cache, 5000);
        assert_eq!(fee_per_byte(&cache).unwrap(), 5000);
    }

    #[test]
    fn set_storage_price_validation_bounds() {
        // C# SetStoragePrice accepts [1, MaxStoragePrice] and rejects outside.
        assert!(validate_storage_price(1).is_ok());
        assert!(validate_storage_price(MAX_STORAGE_PRICE).is_ok());
        assert!(validate_storage_price(0).is_err());
        assert!(validate_storage_price(MAX_STORAGE_PRICE + 1).is_err());
    }

    #[test]
    fn storage_price_write_then_read_round_trips() {
        let cache = DataCache::new(false);
        put_storage_price(&cache, 250_000);
        assert_eq!(storage_price(&cache).unwrap(), 250_000);
        put_storage_price(&cache, 1_000_000);
        assert_eq!(storage_price(&cache).unwrap(), 1_000_000);
    }

    #[test]
    fn set_milliseconds_per_block_validation_bounds() {
        // C# SetMillisecondsPerBlock accepts [1, MaxMillisecondsPerBlock].
        assert!(validate_milliseconds_per_block(1).is_ok());
        assert!(validate_milliseconds_per_block(MAX_MILLISECONDS_PER_BLOCK).is_ok());
        assert!(validate_milliseconds_per_block(0).is_err());
        assert!(validate_milliseconds_per_block(MAX_MILLISECONDS_PER_BLOCK + 1).is_err());
    }

    #[test]
    fn milliseconds_per_block_write_persists_to_storage() {
        let cache = DataCache::new(false);
        put_milliseconds_per_block(&cache, 7_000);
        // Read back the raw storage value (the engine-aware getter adds the
        // ProtocolSettings default, which isn't needed once a value is stored).
        assert_eq!(
            crate::read_storage_int(&cache, PolicyContract::ID, PREFIX_MILLISECONDS_PER_BLOCK, 0)
                .unwrap(),
            7_000
        );
    }

    #[test]
    fn max_chain_param_setter_range_bounds() {
        // C# MaxMaxValidUntilBlockIncrement = 86400, MaxMaxTraceableBlocks = 2102400.
        assert!(validate_max_valid_until_block_increment(1).is_ok());
        assert!(validate_max_valid_until_block_increment(MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT).is_ok());
        assert!(validate_max_valid_until_block_increment(0).is_err());
        assert!(validate_max_valid_until_block_increment(MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT + 1).is_err());

        assert!(validate_max_traceable_blocks(1).is_ok());
        assert!(validate_max_traceable_blocks(MAX_MAX_TRACEABLE_BLOCKS).is_ok());
        assert!(validate_max_traceable_blocks(0).is_err());
        assert!(validate_max_traceable_blocks(MAX_MAX_TRACEABLE_BLOCKS + 1).is_err());
    }

    #[test]
    fn max_chain_param_writes_persist_to_storage() {
        let cache = DataCache::new(false);
        put_max_valid_until_block_increment(&cache, 5_000);
        assert_eq!(
            crate::read_storage_int(
                &cache,
                PolicyContract::ID,
                PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT,
                0
            )
            .unwrap(),
            5_000
        );
        put_max_traceable_blocks(&cache, 1_000_000);
        assert_eq!(
            crate::read_storage_int(&cache, PolicyContract::ID, PREFIX_MAX_TRACEABLE_BLOCKS, 0)
                .unwrap(),
            1_000_000
        );
    }

    #[test]
    fn is_blocked_checks_storage_existence() {
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[3u8; 20]).unwrap();
        let key = {
            let mut k = vec![PREFIX_BLOCKED_ACCOUNT];
            k.extend_from_slice(&account.to_bytes());
            StorageKey::new(PolicyContract::ID, k)
        };
        // Not blocked until a record exists.
        assert!(cache.get(&key).is_none());
        cache.add(key.clone(), StorageItem::from_bytes(vec![]));
        assert!(cache.get(&key).is_some());
    }

    #[test]
    fn fee_per_byte_reads_storage_with_default() {
        let cache = DataCache::new(false);
        // Absent -> default 1000 (C# writes this at initialization).
        assert_eq!(fee_per_byte(&cache).unwrap(), 1000);

        // A configured value is read back from the BigInteger storage item.
        let key = StorageKey::new(PolicyContract::ID, vec![PREFIX_FEE_PER_BYTE]);
        cache.add(key, StorageItem::from_bytes(BigInt::from(4242).to_signed_bytes_le()));
        assert_eq!(fee_per_byte(&cache).unwrap(), 4242);
    }

    #[test]
    fn storage_price_reads_storage_with_default() {
        let cache = DataCache::new(false);
        assert_eq!(storage_price(&cache).unwrap(), DEFAULT_STORAGE_PRICE);

        let key = StorageKey::new(PolicyContract::ID, vec![PREFIX_STORAGE_PRICE]);
        cache.add(key, StorageItem::from_bytes(BigInt::from(250_000).to_signed_bytes_le()));
        assert_eq!(storage_price(&cache).unwrap(), 250_000);
    }
}
