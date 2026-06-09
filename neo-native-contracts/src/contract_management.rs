//! ContractManagement native contract.
//!
//! Concrete implementation of the read-side surface plus the `destroy`
//! writer of the ContractManagement native contract. The deploy /
//! update mutating surface (NEF + manifest validation) lives in the
//! `neo-blockchain` reth-style service (which writes the storage
//! entries this module reads), but the read surface (look up a
//! deployed contract by hash) is consumed by oracle service, RPC, the
//! application engine, and the tokens tracker, so it lives here so all
//! those crates can share it without depending on `neo-blockchain`.

use crate::hashes::CONTRACT_MANAGEMENT_HASH;
use neo_config::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, ContractState, Interoperable, NativeContract, NativeMethod};
use neo_io::{MemoryReader, Serializable};
use neo_primitives::{CallFlags, ContractParameterType, FindOptions, UInt160};
use neo_serialization::BinarySerializer;
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::any::Any;
use std::sync::LazyLock;

/// Storage prefix for the minimum-deployment-fee setting (C#
/// `ContractManagement.Prefix_MinimumDeploymentFee`).
const PREFIX_MINIMUM_DEPLOYMENT_FEE: u8 = 20;
/// C# default minimum deployment fee: 10 GAS, in datoshi.
const DEFAULT_MINIMUM_DEPLOYMENT_FEE: i64 = 10_00000000;

/// Storage prefix for the per-contract record (matches C#
/// `ContractManagement.PREFIX_CONTRACT`).
const PREFIX_CONTRACT: u8 = 8;
/// Storage prefix for the contract-id → hash index (matches C#
/// `ContractManagement.PREFIX_CONTRACT_HASH`).
const PREFIX_CONTRACT_HASH: u8 = 12;

/// C# `PolicyContract.Prefix_BlockedAccount` — written cross-natively here by
/// `destroy` (C# `ContractManagement.Destroy` → `Policy.BlockAccountInternal`).
const POLICY_PREFIX_BLOCKED_ACCOUNT: u8 = 15;
/// C# `PolicyContract.Prefix_WhitelistedFeeContracts` — cleaned cross-natively
/// here by `destroy` (C# `ContractManagement.Destroy` → `Policy.CleanWhitelist`).
const POLICY_PREFIX_WHITELISTED_FEE_CONTRACTS: u8 = 16;

/// Lazily-initialised script-hash handle for the ContractManagement contract.
pub static CONTRACT_MANAGEMENT_HASH_REF: LazyLock<UInt160> =
    LazyLock::new(|| *CONTRACT_MANAGEMENT_HASH);

/// Static accessor for the ContractManagement native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct ContractManagement;

impl ContractManagement {
    /// Stable native contract id (matches C# `ContractManagement.Id`).
    pub const ID: i32 = -1;

    /// Constructs a new `ContractManagement` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the ContractManagement contract.
    pub fn hash(&self) -> UInt160 {
        *CONTRACT_MANAGEMENT_HASH_REF
    }

    /// Returns the script hash of the ContractManagement contract (static).
    pub fn script_hash() -> UInt160 {
        *CONTRACT_MANAGEMENT_HASH_REF
    }

    /// Looks up a deployed contract by its script hash.
    ///
    /// Reads the per-contract record (`prefix 8` + `hash.to_bytes()`)
    /// previously written by `ContractManagement.Deploy` /
    /// `ContractManagement.Update` in the blockchain service.
    pub fn get_contract_from_snapshot(
        snapshot: &DataCache,
        hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        let key = StorageKey::new(Self::ID, contract_storage_key(hash));
        let Some(item) = snapshot.get(&key) else {
            return Ok(None);
        };

        let bytes = item.value_bytes().into_owned();
        if bytes.is_empty() {
            return Ok(None);
        }

        let mut reader = MemoryReader::new(&bytes);
        let state = ContractState::deserialize(&mut reader).map_err(|e| {
            CoreError::deserialization(format!("Failed to deserialize contract state: {e}"))
        })?;
        Ok(Some(state))
    }

    /// Looks up a deployed contract by its contract id.
    ///
    /// Reads the contract-id → hash index (`prefix 12` + `id_be_bytes`)
    /// then dereferences the resulting hash via `get_contract_from_snapshot`.
    pub fn get_contract_by_id_from_snapshot(
        snapshot: &DataCache,
        id: i32,
    ) -> CoreResult<Option<ContractState>> {
        let id_key = StorageKey::new(Self::ID, contract_id_storage_key(id));
        let hash_bytes = match snapshot.get(&id_key) {
            Some(item) => item.value_bytes().into_owned(),
            None => {
                // Fall back to the legacy LE encoding for older snapshots.
                let legacy = StorageKey::new(Self::ID, contract_id_storage_key_legacy(id));
                match snapshot.get(&legacy) {
                    Some(item) => item.value_bytes().into_owned(),
                    None => return Ok(None),
                }
            }
        };

        if hash_bytes.is_empty() {
            return Ok(None);
        }

        let hash = UInt160::from_bytes(&hash_bytes).map_err(|e| {
            CoreError::invalid_data(format!("Invalid contract hash bytes: {e}"))
        })?;
        Self::get_contract_from_snapshot(snapshot, &hash)
    }

    /// Checks whether a contract is deployed in the given snapshot.
    pub fn is_contract(snapshot: &DataCache, hash: &UInt160) -> bool {
        let key = StorageKey::new(Self::ID, contract_storage_key(hash));
        snapshot.get(&key).is_some()
    }
}

#[inline]
fn contract_storage_key(hash: &UInt160) -> Vec<u8> {
    let mut key = Vec::with_capacity(1 + 20);
    key.push(PREFIX_CONTRACT);
    key.extend_from_slice(&hash.to_bytes());
    key
}

#[inline]
fn contract_id_storage_key(id: i32) -> Vec<u8> {
    let mut key = Vec::with_capacity(1 + 4);
    key.push(PREFIX_CONTRACT_HASH);
    key.extend_from_slice(&id.to_be_bytes());
    key
}

#[inline]
fn contract_id_storage_key_legacy(id: i32) -> Vec<u8> {
    let mut key = Vec::with_capacity(1 + 4);
    key.push(PREFIX_CONTRACT_HASH);
    key.extend_from_slice(&id.to_le_bytes());
    key
}

/// Parses the leading `Hash160` argument shared by `getContract`/`isContract`.
fn parse_hash_arg(args: &[Vec<u8>], method: &str) -> CoreResult<UInt160> {
    let hash_bytes = args.first().ok_or_else(|| {
        CoreError::invalid_operation(format!("ContractManagement::{method} requires a hash"))
    })?;
    UInt160::from_bytes(hash_bytes).map_err(|e| {
        CoreError::invalid_operation(format!("ContractManagement::{method}: bad hash: {e}"))
    })
}

/// C# `ContractAbi.GetMethod(name, pcount) != null`: true when the manifest ABI
/// declares a method named `name` whose parameter count matches `pcount`, where
/// `pcount == -1` matches any count.
fn abi_has_method(manifest: &neo_manifest::ContractManifest, name: &str, pcount: i32) -> bool {
    manifest
        .abi
        .methods
        .iter()
        .any(|m| m.name == name && (pcount == -1 || m.parameters.len() as i32 == pcount))
}

/// Marshals a `ContractState` to the Array return bytes (C# `ToStackItem` +
/// `BinarySerializer`) — shared by `getContract` / `getContractById`. A miss is
/// the caller's responsibility (an empty payload encodes the C# `null`).
fn contract_state_to_bytes(state: &ContractState, method: &str) -> CoreResult<Vec<u8>> {
    let item = state.to_stack_item().map_err(|e| {
        CoreError::invalid_operation(format!("ContractManagement::{method}: stack item: {e}"))
    })?;
    BinarySerializer::serialize(&item, &ExecutionEngineLimits::default()).map_err(|e| {
        CoreError::invalid_operation(format!("ContractManagement::{method}: serialize: {e}"))
    })
}

/// Collects the `Prefix_ContractHash` storage entries (`id -> hash`) in
/// forward-seek order, the backing set for C# `GetContractHashes`'s iterator.
///
/// C# reads the contract id back out of each key
/// (`ReadInt32BigEndian(key.Key[1..])`) and keeps only `id >= 0`, which
/// excludes the native contracts (negative ids; their big-endian
/// two's-complement keys sort after every non-negative id).
fn contract_hash_entries(snapshot: &DataCache) -> Vec<(StorageKey, StorageItem)> {
    let prefix_key = StorageKey::new(ContractManagement::ID, vec![PREFIX_CONTRACT_HASH]);
    snapshot
        .find(Some(&prefix_key), SeekDirection::Forward)
        .filter(|(key, _)| {
            let suffix = key.suffix();
            suffix.len() >= 5
                && i32::from_be_bytes([suffix[1], suffix[2], suffix[3], suffix[4]]) >= 0
        })
        .collect()
}

/// C# `NativeContract.IsNative(hash)`: whether the hash belongs to one of the
/// 11 registered native contracts.
fn is_native_contract_hash(hash: &UInt160) -> bool {
    [
        *crate::hashes::CONTRACT_MANAGEMENT_HASH,
        *crate::hashes::STDLIB_HASH,
        *crate::hashes::CRYPTO_LIB_HASH,
        *crate::hashes::LEDGER_CONTRACT_HASH,
        *crate::hashes::NEO_TOKEN_HASH,
        *crate::hashes::GAS_TOKEN_HASH,
        *crate::hashes::POLICY_CONTRACT_HASH,
        *crate::hashes::ROLE_MANAGEMENT_HASH,
        *crate::hashes::ORACLE_CONTRACT_HASH,
        *crate::hashes::NOTARY_HASH,
        *crate::hashes::TREASURY_HASH,
    ]
    .contains(hash)
}

/// C# `PolicyContract.CleanWhitelist(engine, contract)` (PolicyContract.cs
/// ~368), invoked cross-natively by `ContractManagement.Destroy`: deletes every
/// `Prefix_WhitelistedFeeContracts ++ contract.Hash` entry and emits Policy's
/// `WhitelistFeeChanged` event (`[hash, method, argCount, null]`) per removal.
/// Entries decode as the C# `WhitelistedContract` interoperable
/// `Struct[ContractHash, Method, ArgCount, FixedFee]`.
fn policy_clean_whitelist(
    engine: &mut ApplicationEngine,
    contract: &ContractState,
) -> CoreResult<()> {
    let snapshot = engine.snapshot_cache();
    let mut prefix_bytes = Vec::with_capacity(1 + 20);
    prefix_bytes.push(POLICY_PREFIX_WHITELISTED_FEE_CONTRACTS);
    prefix_bytes.extend_from_slice(&contract.hash.to_bytes());
    let prefix_key = StorageKey::new(crate::PolicyContract::ID, prefix_bytes);
    let entries: Vec<(StorageKey, StorageItem)> = snapshot
        .find(Some(&prefix_key), SeekDirection::Forward)
        .collect();
    for (key, item) in entries {
        snapshot.delete(&key);
        let decoded = BinarySerializer::deserialize(
            &item.value_bytes(),
            &ExecutionEngineLimits::default(),
            None,
        )
        .map_err(|e| {
            CoreError::invalid_operation(format!(
                "ContractManagement::destroy: whitelist entry: {e}"
            ))
        })?;
        let StackItem::Struct(fields) = decoded else {
            return Err(CoreError::invalid_data(
                "whitelisted-contract entry is not a struct",
            ));
        };
        let items = fields.items();
        let method = items
            .get(1)
            .ok_or_else(|| CoreError::invalid_data("whitelisted-contract entry missing method"))?
            .as_bytes()
            .map_err(|e| CoreError::invalid_data(format!("whitelist method: {e}")))?;
        let arg_count = items
            .get(2)
            .ok_or_else(|| CoreError::invalid_data("whitelisted-contract entry missing argCount"))?
            .as_int()
            .map_err(|e| CoreError::invalid_data(format!("whitelist argCount: {e}")))?;
        engine
            .send_notification(
                crate::PolicyContract::script_hash(),
                "WhitelistFeeChanged".to_string(),
                vec![
                    StackItem::from_byte_string(contract.hash.to_bytes()),
                    StackItem::from_byte_string(method),
                    StackItem::from_int(arg_count),
                    StackItem::Null,
                ],
            )
            .map_err(|e| {
                CoreError::invalid_operation(format!("ContractManagement::destroy: notify: {e}"))
            })?;
    }
    Ok(())
}

/// C# `SetMinimumDeploymentFee` storage effect: overwrite
/// `Prefix_MinimumDeploymentFee` (`GetAndChange(...).Set(value)`). The key is
/// genesis-initialised, so `update` (= C# GetAndChange) is the correct primitive;
/// the value is stored as the full signed-LE BigInteger (the C# parameter is
/// `BigInteger`, not `long`).
fn put_minimum_deployment_fee(snapshot: &DataCache, value: &BigInt) {
    snapshot.update(
        StorageKey::new(
            ContractManagement::ID,
            vec![PREFIX_MINIMUM_DEPLOYMENT_FEE],
        ),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(value)),
    );
}

static CONTRACT_MANAGEMENT_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        NativeMethod::new(
            "getContract".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Array,
        ),
        NativeMethod::new(
            "getContractById".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Integer],
            ContractParameterType::Array,
        ),
        NativeMethod::new(
            "getMinimumDeploymentFee".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
        // HF_Echidna added the cheap existence check (CpuFee 1<<14).
        NativeMethod::new(
            "isContract".to_string(),
            1 << 14,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfEchidna),
        // HF_Echidna: hasMethod(hash, method, pcount) -> bool.
        NativeMethod::new(
            "hasMethod".to_string(),
            1 << 15,
            true,
            read_states,
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::String,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfEchidna),
        // Committee-gated setter: not safe, States, Integer -> Void.
        NativeMethod::new(
            "setMinimumDeploymentFee".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        ),
        // getContractHashes() -> Iterator over (id, hash) for deployed contracts.
        NativeMethod::new(
            "getContractHashes".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::InteropInterface,
        ),
        // destroy(): the calling contract destroys itself. Not safe,
        // States|AllowNotify, Void (C# ContractManagement.Destroy).
        NativeMethod::new(
            "destroy".to_string(),
            1 << 15,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![],
            ContractParameterType::Void,
        ),
    ]
});

impl NativeContract for ContractManagement {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *CONTRACT_MANAGEMENT_HASH_REF
    }

    fn name(&self) -> &str {
        "ContractManagement"
    }

    fn methods(&self) -> &[NativeMethod] {
        &CONTRACT_MANAGEMENT_METHODS
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// Resolves a deployed contract's state from storage.
    ///
    /// ContractManagement owns the per-contract records, so it backs the
    /// engine's `fetch_contract` storage path (via the native-contract
    /// provider seam): `System.Contract.Call` to any deployed contract —
    /// native or user — resolves its NEF/manifest through here. Delegates to
    /// the read helper used by the `getContract` invoke arm.
    fn lookup_contract_state(
        &self,
        snapshot: &DataCache,
        hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        Self::get_contract_from_snapshot(snapshot, hash)
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        match method {
            "getContract" => {
                let hash = parse_hash_arg(args, "getContract")?;
                // C# `GetContract` returns the ContractState (as an Array via
                // ToStackItem) or null on miss; the native return marshaling
                // encodes a null Array result as an empty payload.
                match Self::get_contract_from_snapshot(&snapshot, &hash)? {
                    Some(state) => contract_state_to_bytes(&state, "getContract"),
                    None => Ok(Vec::new()),
                }
            }
            "getContractById" => {
                // C# `GetContractById` maps the id to a hash via the
                // contract-id index, then returns that ContractState (or null).
                let id = args
                    .first()
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_i32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "ContractManagement::getContractById requires an integer id",
                        )
                    })?;
                match Self::get_contract_by_id_from_snapshot(&snapshot, id)? {
                    Some(state) => contract_state_to_bytes(&state, "getContractById"),
                    None => Ok(Vec::new()),
                }
            }
            "getMinimumDeploymentFee" => {
                let fee = crate::read_storage_int(
                    &snapshot,
                    Self::ID,
                    PREFIX_MINIMUM_DEPLOYMENT_FEE,
                    DEFAULT_MINIMUM_DEPLOYMENT_FEE,
                )?;
                Ok(BigInt::from(fee).to_signed_bytes_le())
            }
            "setMinimumDeploymentFee" => {
                // C#: validate value >= 0 -> AssertCommittee -> overwrite
                // Prefix_MinimumDeploymentFee (stored as the full BigInteger).
                let value = args
                    .first()
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "ContractManagement::setMinimumDeploymentFee requires a value",
                        )
                    })?;
                if value < BigInt::from(0) {
                    return Err(CoreError::invalid_operation(
                        "MinimumDeploymentFee cannot be negative",
                    ));
                }
                let authorized = engine.check_committee_witness().map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "setMinimumDeploymentFee committee check: {e}"
                    ))
                })?;
                if !authorized {
                    return Err(CoreError::invalid_operation(
                        "setMinimumDeploymentFee requires committee authorization",
                    ));
                }
                put_minimum_deployment_fee(&engine.snapshot_cache(), &value);
                Ok(Vec::new())
            }
            "getContractHashes" => {
                // C# GetContractHashes: an iterator over Prefix_ContractHash with
                // FindOptions.RemovePrefix and prefix length 1, yielding
                // Struct[id_bytes, hash]. The 4-byte iterator id is decoded back
                // into an InteropInterface (StorageIterator) by the dispatcher.
                let results = contract_hash_entries(&snapshot);
                let iterator_id = engine
                    .create_storage_iterator_with_options(results, 1, FindOptions::RemovePrefix)
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "ContractManagement::getContractHashes: {e}"
                        ))
                    })?;
                Ok(iterator_id.to_le_bytes().to_vec())
            }
            "isContract" => {
                let hash = parse_hash_arg(args, "isContract")?;
                // C# `IsContract` = snapshot.Contains(key(Prefix_Contract, hash)).
                Ok(vec![u8::from(Self::is_contract(&snapshot, &hash))])
            }
            "hasMethod" => {
                let hash = parse_hash_arg(args, "hasMethod")?;
                let method_name = args
                    .get(1)
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "ContractManagement::hasMethod requires a method name",
                        )
                    })?;
                let pcount = args
                    .get(2)
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_i32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "ContractManagement::hasMethod requires a parameter count",
                        )
                    })?;
                // C#: false if the contract does not exist; otherwise whether its
                // manifest ABI declares the (method, pcount) method.
                let has = match Self::get_contract_from_snapshot(&snapshot, &hash)? {
                    Some(state) => abi_has_method(&state.manifest, &method_name, pcount),
                    None => false,
                };
                Ok(vec![u8::from(has)])
            }
            "destroy" => {
                // C# Destroy (~382): the CALLING contract destroys itself
                // (hash = engine.CallingScriptHash; a missing calling context
                // is the C# null-deref fault).
                let hash = engine.get_calling_script_hash().ok_or_else(|| {
                    CoreError::invalid_operation(
                        "ContractManagement::destroy requires a calling contract",
                    )
                })?;
                // C#: `if (contract is null) return;` — a non-contract caller
                // is a successful no-op.
                let Some(contract) = Self::get_contract_from_snapshot(&snapshot, &hash)? else {
                    return Ok(Vec::new());
                };
                // Delete the per-contract record and the id -> hash index entry.
                snapshot.delete(&StorageKey::new(Self::ID, contract_storage_key(&hash)));
                snapshot.delete(&StorageKey::new(
                    Self::ID,
                    contract_id_storage_key(contract.id),
                ));
                // Delete ALL of the contract's own storage (C# Find over
                // `StorageKey.CreateSearchPrefix(contract.Id, empty)`).
                let search_prefix = StorageKey::new(contract.id, Vec::new());
                let keys: Vec<StorageKey> = snapshot
                    .find(Some(&search_prefix), SeekDirection::Forward)
                    .map(|(key, _)| key)
                    .collect();
                for key in keys {
                    snapshot.delete(&key);
                }
                // C#: `await Policy.BlockAccountInternal(engine, hash)` — lock
                // the destroyed contract (the bool result is discarded).
                crate::policy_contract::block_account_internal(engine, &hash)?;
                // C#: `Policy.CleanWhitelist(engine, contract)`.
                policy_clean_whitelist(engine, &contract)?;
                // Emit the Destroy event with the destroyed hash.
                engine
                    .send_notification(
                        Self::script_hash(),
                        "Destroy".to_string(),
                        vec![StackItem::from_byte_string(hash.to_bytes())],
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "ContractManagement::destroy: notify: {e}"
                        ))
                    })?;
                Ok(Vec::new())
            }
            other => Err(CoreError::invalid_operation(format!(
                "ContractManagement method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::StorageItem;
    use neo_vm::StackItem;

    #[test]
    fn native_contract_surface() {
        let c = ContractManagement::new();
        assert_eq!(NativeContract::id(&c), -1);
        assert_eq!(NativeContract::name(&c), "ContractManagement");
        assert_eq!(NativeContract::hash(&c), *CONTRACT_MANAGEMENT_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "getContract",
                "getContractById",
                "getMinimumDeploymentFee",
                "isContract",
                "hasMethod",
                "setMinimumDeploymentFee",
                "getContractHashes",
                "destroy"
            ]
        );
        // getContractHashes is a safe, ReadStates, no-arg iterator reader.
        let hashes = c.methods().iter().find(|m| m.name == "getContractHashes").unwrap();
        assert!(hashes.safe && hashes.active_in.is_none());
        assert!(hashes.parameters.is_empty());
        assert_eq!(hashes.return_type, ContractParameterType::InteropInterface);
        assert_eq!(hashes.required_call_flags, CallFlags::READ_STATES.bits());
        // The committee-gated setter: not safe, States, Integer -> Void.
        let setter = c
            .methods()
            .iter()
            .find(|m| m.name == "setMinimumDeploymentFee")
            .unwrap();
        assert!(!setter.safe);
        assert_eq!(setter.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(setter.parameters, vec![ContractParameterType::Integer]);
        assert_eq!(setter.return_type, ContractParameterType::Void);
        assert_eq!(setter.cpu_fee, 1 << 15);
        assert!(setter.active_in.is_none());
        let has_method = c.methods().iter().find(|m| m.name == "hasMethod").unwrap();
        assert_eq!(has_method.active_in, Some(Hardfork::HfEchidna));
        assert_eq!(has_method.return_type, ContractParameterType::Boolean);
        assert_eq!(has_method.parameters.len(), 3);

        let get_contract = c.methods().iter().find(|m| m.name == "getContract").unwrap();
        assert_eq!(get_contract.parameters, vec![ContractParameterType::Hash160]);
        assert_eq!(get_contract.return_type, ContractParameterType::Array);
        assert_eq!(get_contract.cpu_fee, 1 << 15);
        assert!(get_contract.safe && get_contract.active_in.is_none());

        let by_id = c.methods().iter().find(|m| m.name == "getContractById").unwrap();
        assert_eq!(by_id.parameters, vec![ContractParameterType::Integer]);
        assert_eq!(by_id.return_type, ContractParameterType::Array);
        assert_eq!(by_id.cpu_fee, 1 << 15);

        let is_contract = c.methods().iter().find(|m| m.name == "isContract").unwrap();
        assert_eq!(is_contract.parameters, vec![ContractParameterType::Hash160]);
        assert_eq!(is_contract.return_type, ContractParameterType::Boolean);
        assert_eq!(is_contract.cpu_fee, 1 << 14);
        assert_eq!(is_contract.active_in, Some(Hardfork::HfEchidna));

        // destroy(): not safe, States|AllowNotify, no params, Void, no hardfork
        // (C# [ContractMethod(CpuFee = 1 << 15,
        // RequiredCallFlags = CallFlags.States | CallFlags.AllowNotify)]).
        let destroy = c.methods().iter().find(|m| m.name == "destroy").unwrap();
        assert!(!destroy.safe);
        assert_eq!(
            destroy.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert!(destroy.parameters.is_empty());
        assert_eq!(destroy.return_type, ContractParameterType::Void);
        assert_eq!(destroy.cpu_fee, 1 << 15);
        assert!(destroy.active_in.is_none());
    }

    #[test]
    fn get_contract_miss_returns_none() {
        // C# `GetContract` returns null for an unknown hash; the invoke arm maps
        // `None` to an empty payload, which the engine decodes to StackItem::Null.
        let cache = DataCache::new(false);
        let hash = UInt160::from_bytes(&[7u8; 20]).unwrap();
        assert!(ContractManagement::get_contract_from_snapshot(&cache, &hash)
            .unwrap()
            .is_none());
    }

    #[test]
    fn get_contract_by_id_miss_returns_none() {
        // C# `GetContractById` returns null when the id has no hash-index entry;
        // the invoke arm maps that to an empty payload (StackItem::Null).
        let cache = DataCache::new(false);
        assert!(ContractManagement::get_contract_by_id_from_snapshot(&cache, 42)
            .unwrap()
            .is_none());
    }

    #[test]
    fn contract_hash_entries_scopes_to_prefix_contract_hash() {
        let cache = DataCache::new(false);
        // Two Prefix_ContractHash entries (id -> hash) plus an unrelated
        // Prefix_Contract entry that must NOT appear in the iterator's backing set.
        let mut k1 = vec![PREFIX_CONTRACT_HASH];
        k1.extend_from_slice(&1i32.to_be_bytes());
        let mut k2 = vec![PREFIX_CONTRACT_HASH];
        k2.extend_from_slice(&2i32.to_be_bytes());
        cache.add(
            StorageKey::new(ContractManagement::ID, k1),
            StorageItem::from_bytes(vec![0xAA; 20]),
        );
        cache.add(
            StorageKey::new(ContractManagement::ID, k2),
            StorageItem::from_bytes(vec![0xBB; 20]),
        );
        cache.add(
            StorageKey::new(ContractManagement::ID, contract_storage_key(&UInt160::zero())),
            StorageItem::from_bytes(vec![1]),
        );

        let entries = contract_hash_entries(&cache);
        assert_eq!(entries.len(), 2, "only Prefix_ContractHash entries are included");
        // Forward-seek order: id 1 before id 2 (big-endian id keys sort ascending).
        assert_eq!(entries[0].1.value_bytes().to_vec(), vec![0xAA; 20]);
        assert_eq!(entries[1].1.value_bytes().to_vec(), vec![0xBB; 20]);
    }

    #[test]
    fn contract_hash_entries_skips_native_negative_ids() {
        // C# GetContractHashes filters `ReadInt32BigEndian(key.Key[1..]) >= 0`:
        // native contracts (negative ids) never appear in the iterator.
        let cache = DataCache::new(false);
        for id in [-1i32, -11] {
            let mut key = vec![PREFIX_CONTRACT_HASH];
            key.extend_from_slice(&id.to_be_bytes());
            cache.add(
                StorageKey::new(ContractManagement::ID, key),
                StorageItem::from_bytes(vec![0xCC; 20]),
            );
        }
        let mut user = vec![PREFIX_CONTRACT_HASH];
        user.extend_from_slice(&1i32.to_be_bytes());
        cache.add(
            StorageKey::new(ContractManagement::ID, user),
            StorageItem::from_bytes(vec![0xDD; 20]),
        );

        let entries = contract_hash_entries(&cache);
        assert_eq!(entries.len(), 1, "native (negative-id) entries are skipped");
        assert_eq!(entries[0].1.value_bytes().to_vec(), vec![0xDD; 20]);
        // id 0 is the boundary: C# keeps `Id >= 0`.
        let mut zero = vec![PREFIX_CONTRACT_HASH];
        zero.extend_from_slice(&0i32.to_be_bytes());
        cache.add(
            StorageKey::new(ContractManagement::ID, zero),
            StorageItem::from_bytes(vec![0xEE; 20]),
        );
        assert_eq!(contract_hash_entries(&cache).len(), 2);
    }

    #[test]
    fn get_contract_by_id_round_trips_through_the_id_index() {
        // Deploy-shaped fixture: the per-contract record (prefix 8) plus the
        // big-endian id -> hash index entry (prefix 12), as written by C#
        // Deploy; GetContractById resolves the id through the index and then
        // dereferences the hash.
        let cache = DataCache::new(false);
        let hash = UInt160::from_bytes(&[0x42u8; 20]).unwrap();
        let state = ContractState::new_native(7, hash, "TestUserContract".to_string());
        let mut writer = neo_io::BinaryWriter::new();
        state.serialize(&mut writer).expect("serialize contract state");
        cache.add(
            StorageKey::new(ContractManagement::ID, contract_storage_key(&hash)),
            StorageItem::from_bytes(writer.to_bytes()),
        );
        cache.add(
            StorageKey::new(ContractManagement::ID, contract_id_storage_key(7)),
            StorageItem::from_bytes(hash.to_bytes().to_vec()),
        );

        let fetched = ContractManagement::get_contract_by_id_from_snapshot(&cache, 7)
            .unwrap()
            .expect("id 7 resolves to the deployed contract");
        assert_eq!(fetched.id, 7);
        assert_eq!(fetched.hash, hash);
        // A different id still misses.
        assert!(ContractManagement::get_contract_by_id_from_snapshot(&cache, 8)
            .unwrap()
            .is_none());
    }

    #[test]
    fn has_method_resolves_contract_from_snapshot() {
        use neo_manifest::{ContractMethodDescriptor, ContractParameterDefinition};
        // The hasMethod invoke arm = GetContract(hash) -> Abi.GetMethod(name,
        // pcount) != null; exercise the same composition over a seeded record.
        let cache = DataCache::new(false);
        let hash = UInt160::from_bytes(&[0x51u8; 20]).unwrap();
        let mut state = ContractState::new_native(9, hash, "HasMethodFixture".to_string());
        state.manifest.abi.methods.push(ContractMethodDescriptor {
            name: "transfer".to_string(),
            parameters: vec![ContractParameterDefinition::default(); 4],
            ..Default::default()
        });
        let mut writer = neo_io::BinaryWriter::new();
        state.serialize(&mut writer).expect("serialize contract state");
        cache.add(
            StorageKey::new(ContractManagement::ID, contract_storage_key(&hash)),
            StorageItem::from_bytes(writer.to_bytes()),
        );

        let fetched = ContractManagement::get_contract_from_snapshot(&cache, &hash)
            .unwrap()
            .expect("contract record resolves");
        // Positive: exact pcount and the -1 wildcard.
        assert!(abi_has_method(&fetched.manifest, "transfer", 4));
        assert!(abi_has_method(&fetched.manifest, "transfer", -1));
        // Negative: wrong pcount / unknown name.
        assert!(!abi_has_method(&fetched.manifest, "transfer", 3));
        assert!(!abi_has_method(&fetched.manifest, "balanceOf", -1));
        // Missing contract -> C# returns false before any ABI lookup.
        let absent = UInt160::from_bytes(&[0x52u8; 20]).unwrap();
        assert!(ContractManagement::get_contract_from_snapshot(&cache, &absent)
            .unwrap()
            .is_none());
    }

    #[test]
    fn is_native_contract_hash_covers_all_eleven_natives() {
        for native in [
            *crate::hashes::CONTRACT_MANAGEMENT_HASH,
            *crate::hashes::STDLIB_HASH,
            *crate::hashes::CRYPTO_LIB_HASH,
            *crate::hashes::LEDGER_CONTRACT_HASH,
            *crate::hashes::NEO_TOKEN_HASH,
            *crate::hashes::GAS_TOKEN_HASH,
            *crate::hashes::POLICY_CONTRACT_HASH,
            *crate::hashes::ROLE_MANAGEMENT_HASH,
            *crate::hashes::ORACLE_CONTRACT_HASH,
            *crate::hashes::NOTARY_HASH,
            *crate::hashes::TREASURY_HASH,
        ] {
            assert!(is_native_contract_hash(&native));
        }
        let user = UInt160::from_bytes(&[0x99u8; 20]).unwrap();
        assert!(!is_native_contract_hash(&user));
    }

    #[test]
    fn policy_blocked_account_key_matches_policy_layout() {
        // The cross-native blocked-account key must match PolicyContract's own
        // layout: (PolicyContract.ID, [Prefix_BlockedAccount(15), account]).
        let account = UInt160::from_bytes(&[0x77u8; 20]).unwrap();
        let key = crate::policy_contract::blocked_account_key(&account);
        assert_eq!(key.id, crate::PolicyContract::ID);
        assert_eq!(key.suffix()[0], POLICY_PREFIX_BLOCKED_ACCOUNT);
        assert_eq!(&key.suffix()[1..], account.to_bytes().as_slice());
    }

    #[test]
    fn set_minimum_deployment_fee_write_round_trips() {
        // The setter's storage effect (overwrite Prefix_MinimumDeploymentFee) is
        // observed by the getMinimumDeploymentFee reader, matching C#
        // GetAndChange(...).Set(value).
        let cache = DataCache::new(false);
        assert_eq!(
            crate::read_storage_int(
                &cache,
                ContractManagement::ID,
                PREFIX_MINIMUM_DEPLOYMENT_FEE,
                DEFAULT_MINIMUM_DEPLOYMENT_FEE
            )
            .unwrap(),
            DEFAULT_MINIMUM_DEPLOYMENT_FEE
        );
        // Zero is permitted (C# rejects only value < 0).
        put_minimum_deployment_fee(&cache, &BigInt::from(0));
        assert_eq!(
            crate::read_storage_int(
                &cache,
                ContractManagement::ID,
                PREFIX_MINIMUM_DEPLOYMENT_FEE,
                DEFAULT_MINIMUM_DEPLOYMENT_FEE
            )
            .unwrap(),
            0
        );
        // Overwrite with a positive fee (GetAndChange semantics).
        put_minimum_deployment_fee(&cache, &BigInt::from(25_00000000i64));
        assert_eq!(
            crate::read_storage_int(
                &cache,
                ContractManagement::ID,
                PREFIX_MINIMUM_DEPLOYMENT_FEE,
                DEFAULT_MINIMUM_DEPLOYMENT_FEE
            )
            .unwrap(),
            25_00000000
        );
    }

    #[test]
    fn abi_has_method_matches_name_and_pcount() {
        use neo_manifest::{
            ContractManifest, ContractMethodDescriptor, ContractParameterDefinition,
        };
        let mut manifest = ContractManifest::new("test".to_string());
        manifest.abi.methods.push(ContractMethodDescriptor {
            name: "transfer".to_string(),
            parameters: vec![ContractParameterDefinition::default(); 4],
            ..Default::default()
        });

        // Exact (name, count) match.
        assert!(abi_has_method(&manifest, "transfer", 4));
        // Wrong count -> no match.
        assert!(!abi_has_method(&manifest, "transfer", 3));
        // pcount == -1 matches any count.
        assert!(abi_has_method(&manifest, "transfer", -1));
        // Unknown name -> no match.
        assert!(!abi_has_method(&manifest, "balanceOf", -1));
        // Empty manifest -> no match.
        assert!(!abi_has_method(&ContractManifest::new("e".to_string()), "transfer", -1));
    }

    #[test]
    fn is_contract_checks_storage_existence() {
        let cache = DataCache::new(false);
        let hash = UInt160::from_bytes(&[8u8; 20]).unwrap();
        assert!(!ContractManagement::is_contract(&cache, &hash));
        cache.add(
            StorageKey::new(ContractManagement::ID, contract_storage_key(&hash)),
            StorageItem::from_bytes(vec![1]),
        );
        assert!(ContractManagement::is_contract(&cache, &hash));
    }

    #[test]
    fn contract_state_marshals_to_five_element_array() {
        // getContract's hit path serializes ContractState.to_stack_item() via the
        // BinarySerializer; the result must be a 5-field Array (id, updateCounter,
        // hash, nef, manifest) per C# ContractState.ToStackItem.
        let state = ContractState::default();
        let item = state.to_stack_item().unwrap();
        let bytes = BinarySerializer::serialize(&item, &ExecutionEngineLimits::default()).unwrap();
        assert!(!bytes.is_empty());
        let decoded =
            BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None).unwrap();
        match decoded {
            StackItem::Array(array) => assert_eq!(array.items().len(), 5),
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn minimum_deployment_fee_reads_storage_with_default() {
        let cache = DataCache::new(false);
        assert_eq!(
            crate::read_storage_int(
                &cache,
                ContractManagement::ID,
                PREFIX_MINIMUM_DEPLOYMENT_FEE,
                DEFAULT_MINIMUM_DEPLOYMENT_FEE
            )
            .unwrap(),
            DEFAULT_MINIMUM_DEPLOYMENT_FEE
        );

        let key = StorageKey::new(ContractManagement::ID, vec![PREFIX_MINIMUM_DEPLOYMENT_FEE]);
        cache.add(key, StorageItem::from_bytes(BigInt::from(5_00000000).to_signed_bytes_le()));
        assert_eq!(
            crate::read_storage_int(
                &cache,
                ContractManagement::ID,
                PREFIX_MINIMUM_DEPLOYMENT_FEE,
                DEFAULT_MINIMUM_DEPLOYMENT_FEE
            )
            .unwrap(),
            5_00000000
        );
    }
}

/// Engine-level tests for `destroy` and its `Policy.BlockAccountInternal` /
/// `Policy.CleanWhitelist` ports, using the witness-gated script-execution
/// harness proven in `neo_token::governance_writer_tests`.
#[cfg(test)]
mod destroy_engine_tests {
    use super::*;
    use neo_config::ProtocolSettings;
    use neo_execution::native_contract::build_native_contract_state;
    use neo_io::BinaryWriter;
    use neo_payloads::signer::Signer;
    use neo_payloads::transaction::Transaction;
    use neo_payloads::witness::Witness;
    use neo_payloads::{Block, BlockHeader};
    use neo_primitives::{TriggerType, Verifiable, WitnessScope};
    use neo_script_builder::ScriptBuilder;
    use neo_vm_rs::VmState;
    use std::sync::Arc;

    /// Writes a serialized contract record under `Prefix_Contract ++ hash`.
    fn put_contract_record(cache: &DataCache, state: &ContractState) {
        let mut writer = BinaryWriter::new();
        state.serialize(&mut writer).expect("serialize contract state");
        cache.add(
            StorageKey::new(ContractManagement::ID, contract_storage_key(&state.hash)),
            StorageItem::from_bytes(writer.to_bytes()),
        );
    }

    /// Builds the entry script `System.Contract.Call(CM, "destroy", [])`.
    fn destroy_script() -> Vec<u8> {
        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(0);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push("destroy".as_bytes());
        builder.emit_push(&ContractManagement::script_hash().to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");
        builder.to_array()
    }

    fn engine_for(
        snapshot: Arc<DataCache>,
        persisting_block: Option<Block>,
        settings: ProtocolSettings,
    ) -> ApplicationEngine {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);
        ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            snapshot,
            persisting_block,
            settings,
            100_00000000,
            None,
        )
        .expect("engine builds")
    }

    #[test]
    fn destroy_removes_record_index_storage_and_blocks_hash() {
        crate::install();
        let cache = DataCache::new(false);
        // Seed the ContractManagement native record so System.Contract.Call
        // resolves the callee.
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );

        // The entry script IS the calling contract: pin its hash, then deploy
        // a user contract under that hash (record + id index + one storage
        // row + one Policy whitelist entry).
        let script = destroy_script();
        let self_hash = UInt160::from_script(&script);
        let user = ContractState::new_native(7, self_hash, "SelfDestructFixture".to_string());
        put_contract_record(&cache, &user);
        let index_key = StorageKey::new(ContractManagement::ID, contract_id_storage_key(7));
        cache.add(index_key.clone(), StorageItem::from_bytes(self_hash.to_bytes().to_vec()));
        let user_row = StorageKey::new(7, vec![0x01]);
        cache.add(user_row.clone(), StorageItem::from_bytes(vec![0xEE]));
        // A whitelist entry for the contract (C# WhitelistedContract
        // Struct[ContractHash, Method, ArgCount, FixedFee]) that CleanWhitelist
        // must remove and report.
        let mut wl_suffix = vec![POLICY_PREFIX_WHITELISTED_FEE_CONTRACTS];
        wl_suffix.extend_from_slice(&self_hash.to_bytes());
        wl_suffix.extend_from_slice(&0i32.to_be_bytes());
        let wl_key = StorageKey::new(crate::PolicyContract::ID, wl_suffix);
        let wl_value = BinarySerializer::serialize(
            &StackItem::from_struct(vec![
                StackItem::from_byte_string(self_hash.to_bytes()),
                StackItem::from_byte_string("transfer".as_bytes().to_vec()),
                StackItem::from_int(4),
                StackItem::from_int(0),
            ]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        cache.add(wl_key.clone(), StorageItem::from_bytes(wl_value));
        let snapshot = Arc::new(cache);

        // HF_Faun is unscheduled on default (MainNet) settings, so this runs
        // the pre-Faun BlockAccountInternal branch (empty blocked value).
        let mut engine =
            engine_for(Arc::clone(&snapshot), None, ProtocolSettings::default());
        engine
            .load_script(script, CallFlags::ALL, Some(self_hash))
            .expect("script loads");
        assert_eq!(engine.execute_allow_fault(), VmState::HALT, "destroy must HALT");

        // The contract record, id index, and contract storage are gone.
        assert!(
            snapshot
                .get(&StorageKey::new(ContractManagement::ID, contract_storage_key(&self_hash)))
                .is_none(),
            "contract record deleted"
        );
        assert!(snapshot.get(&index_key).is_none(), "id->hash index entry deleted");
        assert!(snapshot.get(&user_row).is_none(), "contract storage deleted");
        // The destroyed hash is locked via Policy's blocked-account entry,
        // pre-Faun with an EMPTY value (C# StorageItem([])).
        let blocked = snapshot
            .get(&crate::policy_contract::blocked_account_key(&self_hash))
            .expect("destroyed contract is blocked");
        assert!(blocked.value_bytes().is_empty(), "pre-Faun blocked value is empty");
        // The whitelist entry was cleaned.
        assert!(snapshot.get(&wl_key).is_none(), "whitelist entry deleted");

        // Events: Policy's WhitelistFeeChanged for the cleaned entry, then
        // ContractManagement's Destroy with the destroyed hash.
        let notifications = engine.notifications();
        let destroy_event = notifications
            .iter()
            .find(|n| n.event_name == "Destroy")
            .expect("Destroy event emitted");
        assert_eq!(destroy_event.script_hash, ContractManagement::script_hash());
        assert_eq!(
            destroy_event.state[0].as_bytes().unwrap(),
            self_hash.to_bytes().to_vec()
        );
        let wl_event = notifications
            .iter()
            .find(|n| n.event_name == "WhitelistFeeChanged")
            .expect("WhitelistFeeChanged event emitted");
        assert_eq!(wl_event.script_hash, crate::PolicyContract::script_hash());
        assert_eq!(wl_event.state[1].as_bytes().unwrap(), b"transfer".to_vec());
        assert_eq!(wl_event.state[2].as_int().unwrap(), BigInt::from(4));
        assert!(matches!(wl_event.state[3], StackItem::Null));
    }

    #[test]
    fn destroy_is_a_noop_for_a_non_contract_caller() {
        crate::install();
        let cache = DataCache::new(false);
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );
        let script = destroy_script();
        let self_hash = UInt160::from_script(&script);
        let snapshot = Arc::new(cache);

        // No contract record for the calling script: C# `if (contract is null)
        // return;` — a successful no-op that writes nothing.
        let mut engine =
            engine_for(Arc::clone(&snapshot), None, ProtocolSettings::default());
        engine
            .load_script(script, CallFlags::ALL, Some(self_hash))
            .expect("script loads");
        assert_eq!(engine.execute_allow_fault(), VmState::HALT, "no-op destroy HALTs");
        assert!(
            snapshot.get(&crate::policy_contract::blocked_account_key(&self_hash)).is_none(),
            "no blocked-account entry for a no-op destroy"
        );
        assert!(
            engine.notifications().iter().all(|n| n.event_name != "Destroy"),
            "no Destroy event for a no-op destroy"
        );
    }

    #[test]
    fn block_account_internal_faun_writes_timestamp_and_is_idempotent() {
        crate::install();
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfFaun, 0);
        let mut header = BlockHeader::default();
        header.set_index(1);
        header.set_timestamp(1_700_000_123_456);
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = engine_for(
            Arc::clone(&snapshot),
            Some(Block::from_parts(header, vec![])),
            settings,
        );

        let account = UInt160::from_bytes(&[0x33u8; 20]).unwrap();
        // First block: post-Faun the entry stores GetTime() (the persisting
        // block's timestamp) for Policy's recoverFund.
        assert!(crate::policy_contract::block_account_internal(&mut engine, &account).unwrap());
        let item = snapshot
            .get(&crate::policy_contract::blocked_account_key(&account))
            .expect("blocked entry written");
        assert_eq!(
            BigInt::from_signed_bytes_le(&item.value_bytes()),
            BigInt::from(1_700_000_123_456i64)
        );
        // Already blocked -> false, nothing rewritten (C# returns early).
        assert!(!crate::policy_contract::block_account_internal(&mut engine, &account).unwrap());
    }

    #[test]
    fn block_account_internal_rejects_native_hashes() {
        crate::install();
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = engine_for(Arc::clone(&snapshot), None, ProtocolSettings::default());
        // C#: "Cannot block a native contract."
        let neo_hash = *crate::hashes::NEO_TOKEN_HASH;
        let err = crate::policy_contract::block_account_internal(&mut engine, &neo_hash).unwrap_err();
        assert!(err.to_string().contains("native"));
        assert!(snapshot.get(&crate::policy_contract::blocked_account_key(&neo_hash)).is_none());
    }

    #[test]
    fn block_account_internal_faun_runs_vote_transition_for_neo_holders() {
        // C# BlockAccountInternal post-Faun runs NEO.VoteInternal(account,
        // null): for a NEO-holding account the full vote transition executes
        // (here a no-op un-vote — the account votes for nobody), then the
        // blocked entry is written with the persisting block's timestamp.
        crate::install();
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfFaun, 0);
        let mut header = BlockHeader::default();
        header.set_index(1);
        header.set_timestamp(1_700_000_000_000);
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[0x44u8; 20]).unwrap();
        // Seed a NeoToken account state holding 100 NEO.
        let mut neo_key = vec![crate::NEP17_PREFIX_ACCOUNT];
        neo_key.extend_from_slice(&account.to_bytes());
        let neo_state = BinarySerializer::serialize(
            &StackItem::from_struct(vec![
                StackItem::from_int(100),
                StackItem::from_int(0),
                StackItem::Null,
                StackItem::from_int(0),
            ]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        cache.add(
            StorageKey::new(crate::NeoToken::ID, neo_key),
            StorageItem::from_bytes(neo_state),
        );
        let snapshot = Arc::new(cache);
        let mut engine = engine_for(
            Arc::clone(&snapshot),
            Some(Block::from_parts(header, vec![])),
            settings,
        );

        assert!(crate::policy_contract::block_account_internal(&mut engine, &account).unwrap());
        let item = snapshot
            .get(&crate::policy_contract::blocked_account_key(&account))
            .expect("blocked entry written after the vote transition");
        assert_eq!(
            BigInt::from_signed_bytes_le(&item.value_bytes()),
            BigInt::from(1_700_000_000_000i64),
            "entry stores GetTime() for recoverFund"
        );
    }
}
