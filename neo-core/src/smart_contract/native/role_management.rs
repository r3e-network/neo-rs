//! Role Management native contract implementation.
//!
//! Mirrors the behaviour of `Neo.SmartContract.Native.RoleManagement` by
//! persisting designated nodes per role and enforcing committee authorization.

use crate::error::{CoreError as Error, CoreResult as Result};
use crate::hardfork::Hardfork;
use crate::persistence::{DataCache, SeekDirection};
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractParameterDefinition};
use crate::smart_contract::native::{LedgerContract, NativeContract, NativeMethod, Role};
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::ContractParameterType;
use crate::{ECCurve, ECPoint, UInt160};
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use neo_vm::StackItem;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::convert::TryInto;

/// The RoleManagement native contract.
pub struct RoleManagement {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl RoleManagement {
    const ID: i32 = -8;
    const MAX_NODES: usize = 32;
    const CPU_FEE: i64 = 1 << 15;

    /// Creates a new RoleManagement contract.
    pub fn new() -> Self {
        // RoleManagement contract hash: 0x49cf4e5378ffcd4dec034fd98a174c5491e395e2
        let hash = UInt160::parse("0x49cf4e5378ffcd4dec034fd98a174c5491e395e2")
            .expect("Valid RoleManagement contract hash");

        let methods = vec![
            NativeMethod::safe(
                "getDesignatedByRole".to_string(),
                Self::CPU_FEE,
                vec![
                    ContractParameterType::Integer,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Array,
            ),
            NativeMethod::unsafe_method(
                "designateAsRole".to_string(),
                Self::CPU_FEE,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![ContractParameterType::Integer, ContractParameterType::Array],
                ContractParameterType::Void,
            ),
        ];
        let methods = methods
            .into_iter()
            .map(|method| match method.name.as_str() {
                "getDesignatedByRole" => method
                    .with_parameter_names(vec!["role".to_string(), "index".to_string()])
                    .with_required_call_flags(CallFlags::READ_STATES),
                "designateAsRole" => {
                    method.with_parameter_names(vec!["role".to_string(), "nodes".to_string()])
                }
                _ => method,
            })
            .collect();

        Self {
            id: Self::ID,
            hash,
            methods,
        }
    }

    fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            "getDesignatedByRole" => self.get_designated_by_role(engine, args),
            "designateAsRole" => self.designate_as_role(engine, args),
            _ => Err(Error::native_contract(format!(
                "Unknown method: {}",
                method
            ))),
        }
    }

    fn get_designated_by_role(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        let (role, index) = self.parse_role_and_index(args)?;
        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();

        let ledger = LedgerContract::new();
        let current_index = ledger
            .current_index(snapshot_ref)
            .map_err(|err| Error::native_contract(err.to_string()))?;
        if index > current_index.saturating_add(1) {
            return Err(Error::native_contract(format!(
                "Index {} exceeds current index + 1 ({})",
                index,
                current_index.saturating_add(1)
            )));
        }

        match self.find_designation_bytes(snapshot_ref, role, index)? {
            Some(bytes) => Ok(bytes),
            None => self.serialize_public_keys(&[]),
        }
    }

    fn designate_as_role(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if !engine
            .check_committee_witness()
            .map_err(|err| Error::runtime_error(err.to_string()))?
        {
            return Err(Error::invalid_operation(
                "Committee authorization required".to_string(),
            ));
        }

        if args.is_empty() {
            return Err(Error::native_contract(
                "designateAsRole requires role argument".to_string(),
            ));
        }
        let (role, _) = self.parse_role_and_index(&args[..1])?;
        if args.len() < 2 {
            return Err(Error::native_contract(
                "designateAsRole requires role and public keys arguments".to_string(),
            ));
        }

        let mut public_keys = self.parse_public_keys(&args[1])?;
        if public_keys.is_empty() || public_keys.len() > Self::MAX_NODES {
            return Err(Error::native_contract(format!(
                "Nodes count {} must be between 1 and {}",
                public_keys.len(),
                Self::MAX_NODES
            )));
        }
        public_keys.sort();

        let persisting_block = engine
            .persisting_block()
            .ok_or_else(|| Error::invalid_operation("Persisting block is not available"))?;
        let persisting_index = persisting_block.header.index;
        let designation_index = persisting_index
            .checked_add(1)
            .ok_or_else(|| Error::invalid_operation("Block index overflowed"))?;

        let context = engine.get_native_storage_context(&self.hash)?;
        let key_suffix = Self::role_key_suffix(role, designation_index);
        if engine.get_storage_item(&context, &key_suffix).is_some() {
            return Err(Error::invalid_operation(
                "Role already designated at this height".to_string(),
            ));
        }

        let serialized_keys = self.serialize_public_keys(&public_keys)?;
        engine.put_storage_item(&context, &key_suffix, &serialized_keys)?;

        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let previous = match self.find_designation_bytes(
            snapshot_ref,
            role,
            designation_index.saturating_sub(1),
        )? {
            Some(bytes) => bytes,
            None => self.serialize_public_keys(&[])?,
        };

        if engine.is_hardfork_enabled(Hardfork::HfEchidna) {
            let old_keys = self.parse_public_keys(&previous)?;
            let old_nodes = StackItem::from_array(
                old_keys
                    .iter()
                    .map(|key| StackItem::from_byte_string(key.as_bytes().to_vec()))
                    .collect::<Vec<_>>(),
            );
            let new_nodes = StackItem::from_array(
                public_keys
                    .iter()
                    .map(|key| StackItem::from_byte_string(key.as_bytes().to_vec()))
                    .collect::<Vec<_>>(),
            );
            engine
                .send_notification(
                    self.hash,
                    "Designation".to_string(),
                    vec![
                        StackItem::from_int(role as i64),
                        StackItem::from_int(persisting_index as i64),
                        old_nodes,
                        new_nodes,
                    ],
                )
                .map_err(Error::native_contract)?;
        } else {
            engine
                .send_notification(
                    self.hash,
                    "Designation".to_string(),
                    vec![
                        StackItem::from_int(role as i64),
                        StackItem::from_int(persisting_index as i64),
                    ],
                )
                .map_err(Error::native_contract)?;
        }

        Ok(Vec::new())
    }

    fn parse_role_and_index(&self, args: &[Vec<u8>]) -> Result<(Role, u32)> {
        if args.is_empty() {
            return Err(Error::native_contract("Missing role argument"));
        }

        let role_value = BigInt::from_signed_bytes_le(&args[0])
            .to_u8()
            .ok_or_else(|| Error::native_contract("Invalid role argument"))?;
        let role = Role::from_u8(role_value).ok_or_else(|| {
            Error::native_contract(format!("Invalid role identifier: {}", role_value))
        })?;

        let index = if args.len() >= 2 {
            BigInt::from_signed_bytes_le(&args[1])
                .to_u32()
                .ok_or_else(|| {
                    Error::native_contract(
                        "Index argument must be a non-negative 32-bit integer".to_string(),
                    )
                })?
        } else {
            0
        };

        Ok((role, index))
    }

    fn find_designation_bytes(
        &self,
        snapshot: &DataCache,
        role: Role,
        index: u32,
    ) -> Result<Option<Vec<u8>>> {
        let prefix = Self::role_prefix_key(role);
        let iter = snapshot.find(Some(&prefix), SeekDirection::Backward);
        for (key, item) in iter {
            if let Some(designation_index) = Self::parse_designation_index(&key, role) {
                if designation_index <= index {
                    return Ok(Some(item.get_value()));
                }
            }
        }
        Ok(None)
    }

    fn parse_designation_index(key: &StorageKey, role: Role) -> Option<u32> {
        let suffix = key.suffix();
        if suffix.first().copied() != Some(role as u8) || suffix.len() != 5 {
            return None;
        }
        let bytes: [u8; 4] = suffix[1..].try_into().ok()?;
        Some(u32::from_be_bytes(bytes))
    }

    fn role_prefix_key(role: Role) -> StorageKey {
        StorageKey::create(Self::ID, role as u8)
    }

    fn role_key_suffix(role: Role, index: u32) -> Vec<u8> {
        let mut suffix = vec![role as u8];
        suffix.extend_from_slice(&index.to_be_bytes());
        suffix
    }

    /// Gets designated nodes for a role at a specific block index.
    /// This is a public API used by other native contracts like Notary.
    pub fn get_designated_by_role_at(
        &self,
        snapshot: &DataCache,
        role: Role,
        index: u32,
    ) -> Result<Vec<ECPoint>> {
        match self.find_designation_bytes(snapshot, role, index)? {
            Some(bytes) => self.parse_public_keys(&bytes),
            None => Ok(vec![]),
        }
    }

    /// Serializes public keys to bytes.
    pub(crate) fn serialize_public_keys(&self, public_keys: &[ECPoint]) -> Result<Vec<u8>> {
        let items: Vec<StackItem> = public_keys
            .iter()
            .map(|pubkey| StackItem::from_byte_string(pubkey.as_bytes().to_vec()))
            .collect();
        let array = StackItem::from_array(items);
        BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
            .map_err(|e| Error::native_contract(format!("Failed to serialize public keys: {e}")))
    }

    /// Parses public keys from bytes (little-endian count + compressed points).
    fn parse_public_keys(&self, data: &[u8]) -> Result<Vec<ECPoint>> {
        if data.is_empty() {
            return Ok(Vec::new());
        }

        let item = BinarySerializer::deserialize(data, &ExecutionEngineLimits::default(), None)
            .map_err(|e| {
                Error::native_contract(format!("Failed to deserialize public keys: {e}"))
            })?;

        let StackItem::Array(array) = item else {
            return Err(Error::native_contract(
                "Public keys payload must be an array".to_string(),
            ));
        };

        let mut keys = Vec::with_capacity(array.len());
        for element in array.items() {
            let bytes = element
                .as_bytes()
                .map_err(|_| Error::native_contract("Invalid public key item"))?;
            let pubkey = ECPoint::decode_compressed_with_curve(ECCurve::secp256r1(), &bytes)
                .map_err(|e| Error::native_contract(format!("Invalid public key encoding: {e}")))?;
            keys.push(pubkey);
        }

        Ok(keys)
    }
}

impl NativeContract for RoleManagement {
    fn id(&self) -> i32 {
        self.id
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "RoleManagement"
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn events(
        &self,
        settings: &crate::protocol_settings::ProtocolSettings,
        block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        if settings.is_hardfork_enabled(Hardfork::HfEchidna, block_height) {
            vec![ContractEventDescriptor::new(
                "Designation".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "Role".to_string(),
                        ContractParameterType::Integer,
                    )
                    .expect("Designation.Role"),
                    ContractParameterDefinition::new(
                        "BlockIndex".to_string(),
                        ContractParameterType::Integer,
                    )
                    .expect("Designation.BlockIndex"),
                    ContractParameterDefinition::new(
                        "Old".to_string(),
                        ContractParameterType::Array,
                    )
                    .expect("Designation.Old"),
                    ContractParameterDefinition::new(
                        "New".to_string(),
                        ContractParameterType::Array,
                    )
                    .expect("Designation.New"),
                ],
            )
            .expect("Designation event descriptor")]
        } else {
            vec![ContractEventDescriptor::new(
                "Designation".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "Role".to_string(),
                        ContractParameterType::Integer,
                    )
                    .expect("Designation.Role"),
                    ContractParameterDefinition::new(
                        "BlockIndex".to_string(),
                        ContractParameterType::Integer,
                    )
                    .expect("Designation.BlockIndex"),
                ],
            )
            .expect("Designation event descriptor")]
        }
    }

    fn activations(&self) -> Vec<Hardfork> {
        vec![Hardfork::HfEchidna]
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        self.invoke_method(engine, method, args)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Default for RoleManagement {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cryptography::Secp256r1Crypto;
    use crate::network::p2p::payloads::{signer::Signer, transaction::Transaction};
    use crate::smart_contract::trigger_type::TriggerType;
    use crate::smart_contract::StorageItem;
    use crate::witness::Witness;
    use crate::{IVerifiable, WitnessScope};
    use neo_vm::{OpCode, ScriptBuilder};
    use std::sync::Arc;

    fn sample_point(tag: u8) -> ECPoint {
        let private_key = {
            let mut bytes = [0u8; 32];
            bytes[31] = tag.max(1);
            bytes
        };

        let public_key =
            Secp256r1Crypto::derive_public_key(&private_key).expect("derive public key for test");
        ECPoint::decode_compressed_with_curve(ECCurve::secp256r1(), &public_key)
            .expect("valid test key")
    }

    fn make_engine(snapshot: Arc<DataCache>) -> ApplicationEngine {
        const TEST_GAS_LIMIT: i64 = 400_000_000;

        let mut container = Transaction::new();
        container.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
        container.add_witness(Witness::new());
        let script_container: Arc<dyn IVerifiable> = Arc::new(container);

        ApplicationEngine::new(
            TriggerType::Application,
            Some(script_container),
            snapshot,
            None,
            Default::default(),
            TEST_GAS_LIMIT,
            None,
        )
        .expect("engine")
    }

    fn emit_contract_call(
        sb: &mut ScriptBuilder,
        contract_hash: UInt160,
        method: &str,
        mut args: Vec<StackItem>,
    ) {
        let arg_count = args.len();
        for arg in args.drain(..).rev() {
            sb.emit_push_stack_item(arg).expect("emit arg");
        }
        sb.emit_push_int(arg_count as i64);
        sb.emit_opcode(OpCode::PACK);
        sb.emit_push_int(CallFlags::ALL.bits() as i64);
        sb.emit_push_string(method);
        sb.emit_push_byte_array(&contract_hash.to_bytes());
        sb.emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call syscall");
    }

    #[test]
    fn serialize_and_parse_roundtrip() {
        let contract = RoleManagement::new();
        let keys = vec![sample_point(0xAA), sample_point(0xBB)];
        let encoded = contract.serialize_public_keys(&keys).unwrap();
        let decoded = contract.parse_public_keys(&encoded).unwrap();
        assert_eq!(decoded, keys);
    }

    #[test]
    fn find_designation_returns_latest_entry() {
        let contract = RoleManagement::new();
        let cache = DataCache::new(false);
        let role = Role::Oracle;

        let key_old = StorageKey::new(RoleManagement::ID, RoleManagement::role_key_suffix(role, 5));
        let bytes_old = contract
            .serialize_public_keys(&[sample_point(0x10)])
            .unwrap();
        cache.add(key_old, StorageItem::from_bytes(bytes_old.clone()));

        let key_new = StorageKey::new(
            RoleManagement::ID,
            RoleManagement::role_key_suffix(role, 12),
        );
        let bytes_new = contract
            .serialize_public_keys(&[sample_point(0x11)])
            .unwrap();
        cache.add(key_new, StorageItem::from_bytes(bytes_new.clone()));

        let result_before = contract
            .find_designation_bytes(&cache, role, 7)
            .unwrap()
            .expect("entry");
        assert_eq!(result_before, bytes_old);

        let result_after = contract
            .find_designation_bytes(&cache, role, 99)
            .unwrap()
            .expect("entry");
        assert_eq!(result_after, bytes_new);
    }

    #[test]
    fn vm_get_designated_by_role_accepts_compact_integer_index() {
        let contract = RoleManagement::new();
        let snapshot = Arc::new(DataCache::new(false));
        let public_key = sample_point(0x21);
        let encoded = contract
            .serialize_public_keys(std::slice::from_ref(&public_key))
            .unwrap();
        snapshot.add(
            StorageKey::new(
                RoleManagement::ID,
                RoleManagement::role_key_suffix(Role::Oracle, 1),
            ),
            StorageItem::from_bytes(encoded),
        );

        let mut sb = ScriptBuilder::new();
        emit_contract_call(
            &mut sb,
            contract.hash(),
            "getDesignatedByRole",
            vec![
                StackItem::from_int(Role::Oracle as u8 as i64),
                StackItem::from_int(1),
            ],
        );
        sb.emit_opcode(OpCode::RET);

        let mut engine = make_engine(Arc::clone(&snapshot));
        engine
            .load_script(sb.to_array(), CallFlags::ALL, None)
            .expect("load script");
        engine
            .execute()
            .expect("execute role lookup with compact integer index");

        let designated = engine.result_stack().peek(0).unwrap().as_array().unwrap();
        assert_eq!(designated.len(), 1);
        assert_eq!(designated[0].as_bytes().unwrap(), public_key.to_bytes());
    }
}
