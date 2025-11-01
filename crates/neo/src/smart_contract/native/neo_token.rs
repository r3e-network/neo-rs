use super::native_contract::{NativeContract, NativeMethod};
use crate::cryptography::crypto_utils::ECPoint;
use crate::error::{CoreError, CoreResult};
use crate::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::StorageItem;
use crate::UInt160;
use lazy_static::lazy_static;
use neo_vm::{stack_item::StackItem, ExecutionEngineLimits};
use num_bigint::BigInt;
use std::any::Any;

lazy_static! {
    static ref NEO_HASH: UInt160 = Helper::get_contract_hash(&UInt160::zero(), 0, "NeoToken");
}

/// Simplified representation of the NEO native contract exposing the canonical
/// identifiers used throughout the node. Full voting and reward distribution
/// logic will be introduced once the surrounding infrastructure is ported.
pub struct NeoToken {
    methods: Vec<NativeMethod>,
}

impl NeoToken {
    const ID: i32 = -5;
    const SYMBOL: &'static str = "NEO";
    const DECIMALS: u8 = 0;
    const NAME: &'static str = "NeoToken";
    const TOTAL_SUPPLY: i64 = 100_000_000;
    const PREFIX_COMMITTEE: u8 = 14;

    pub fn new() -> Self {
        let methods = vec![
            NativeMethod::safe("symbol".to_string(), 1),
            NativeMethod::safe("decimals".to_string(), 1),
            NativeMethod::safe("totalSupply".to_string(), 1),
        ];

        Self { methods }
    }

    fn total_supply_bytes() -> Vec<u8> {
        let mut bytes = BigInt::from(Self::TOTAL_SUPPLY).to_signed_bytes_le();
        if bytes.is_empty() {
            bytes.push(0);
        }
        bytes
    }

    fn invoke_method(&self, method: &str) -> CoreResult<Vec<u8>> {
        match method {
            "symbol" => Ok(Self::SYMBOL.as_bytes().to_vec()),
            "decimals" => Ok(vec![Self::DECIMALS]),
            "totalSupply" => Ok(Self::total_supply_bytes()),
            _ => Err(CoreError::native_contract(format!(
                "Method not implemented: {}",
                method
            ))),
        }
    }

    pub fn symbol(&self) -> &'static str {
        Self::SYMBOL
    }

    pub fn decimals(&self) -> u8 {
        Self::DECIMALS
    }

    pub fn total_supply(&self) -> BigInt {
        BigInt::from(Self::TOTAL_SUPPLY)
    }

    /// Attempts to read the current committee from the snapshot-backed storage used by the
    /// native NEO contract. Returns `None` when the committee cache has not been populated yet.
    pub fn committee_from_snapshot<S>(&self, snapshot: &S) -> Option<Vec<ECPoint>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = StorageKey::create(Self::ID, Self::PREFIX_COMMITTEE);
        let item = snapshot.try_get(&key)?;
        let bytes = item.get_value();
        let stack_item =
            BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None).ok()?;

        Self::decode_committee_stack_item(stack_item).ok()
    }

    fn decode_committee_stack_item(item: StackItem) -> Result<Vec<ECPoint>, String> {
        use neo_vm::stack_item::StackItem as VmStackItem;

        fn stack_item_to_bytes(item: &VmStackItem) -> Option<Vec<u8>> {
            match item {
                VmStackItem::ByteString(bytes) => Some(bytes.clone()),
                VmStackItem::Buffer(buffer) => Some(buffer.data().to_vec()),
                _ => None,
            }
        }

        fn decode_entry(entry: &VmStackItem) -> Result<Option<ECPoint>, String> {
            let elements: Vec<VmStackItem> = match entry {
                VmStackItem::Struct(structure) => structure.items().to_vec(),
                VmStackItem::Array(array) => array.items().to_vec(),
                _ => return Ok(None),
            };

            let first = elements
                .get(0)
                .ok_or_else(|| "committee entry missing public key".to_string())?;
            let key_bytes = stack_item_to_bytes(first)
                .ok_or_else(|| "committee entry public key must be byte array".to_string())?;
            let point = ECPoint::from_bytes(&key_bytes)
                .map_err(|e| format!("invalid committee public key: {e}"))?;
            Ok(Some(point))
        }

        match item {
            VmStackItem::Array(array) => {
                let mut committee = Vec::with_capacity(array.len());
                for entry in array.items() {
                    if let Some(point) = decode_entry(entry)? {
                        committee.push(point);
                    }
                }
                if committee.is_empty() {
                    Err("committee cache empty".to_string())
                } else {
                    Ok(committee)
                }
            }
            VmStackItem::Struct(structure) => {
                let mut committee = Vec::with_capacity(structure.len());
                for entry in structure.items() {
                    if let Some(point) = decode_entry(entry)? {
                        committee.push(point);
                    }
                }
                if committee.is_empty() {
                    Err("committee cache empty".to_string())
                } else {
                    Ok(committee)
                }
            }
            _ => Err("unexpected committee cache format".to_string()),
        }
    }
}

impl NativeContract for NeoToken {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *NEO_HASH
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn is_active(&self, _settings: &ProtocolSettings, _block_height: u32) -> bool {
        true
    }

    fn invoke(
        &self,
        _engine: &mut ApplicationEngine,
        method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_method(method)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
