//! ContractManagement native contract.
//!
//! Concrete implementation of the read-side surface of the
//! ContractManagement native contract. The full deploy / update /
//! destroy mutating surface lives in the `neo-blockchain` reth-style
//! service (which writes the storage entries this module reads), but
//! the read surface (look up a deployed contract by hash) is consumed
//! by oracle service, RPC, the application engine, and the tokens
//! tracker, so it lives here so all those crates can share it without
//! depending on `neo-blockchain`.

use crate::hashes::CONTRACT_MANAGEMENT_HASH;
use neo_config::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, ContractState, Interoperable, NativeContract, NativeMethod};
use neo_io::{MemoryReader, Serializable};
use neo_primitives::{CallFlags, ContractParameterType, UInt160};
use neo_serialization::BinarySerializer;
use neo_storage::persistence::DataCache;
use neo_storage::StorageKey;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;
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
                    Some(state) => {
                        let item = state.to_stack_item().map_err(|e| {
                            CoreError::invalid_operation(format!(
                                "ContractManagement::getContract: stack item: {e}"
                            ))
                        })?;
                        BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())
                            .map_err(|e| {
                                CoreError::invalid_operation(format!(
                                    "ContractManagement::getContract: serialize: {e}"
                                ))
                            })
                    }
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
            "isContract" => {
                let hash = parse_hash_arg(args, "isContract")?;
                // C# `IsContract` = snapshot.Contains(key(Prefix_Contract, hash)).
                Ok(vec![u8::from(Self::is_contract(&snapshot, &hash))])
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
        assert_eq!(names, ["getContract", "getMinimumDeploymentFee", "isContract"]);

        let get_contract = c.methods().iter().find(|m| m.name == "getContract").unwrap();
        assert_eq!(get_contract.parameters, vec![ContractParameterType::Hash160]);
        assert_eq!(get_contract.return_type, ContractParameterType::Array);
        assert_eq!(get_contract.cpu_fee, 1 << 15);
        assert!(get_contract.safe && get_contract.active_in.is_none());

        let is_contract = c.methods().iter().find(|m| m.name == "isContract").unwrap();
        assert_eq!(is_contract.parameters, vec![ContractParameterType::Hash160]);
        assert_eq!(is_contract.return_type, ContractParameterType::Boolean);
        assert_eq!(is_contract.cpu_fee, 1 << 14);
        assert_eq!(is_contract.active_in, Some(Hardfork::HfEchidna));
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
