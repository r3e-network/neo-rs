//! Role Management native contract implementation.
//!
//! Mirrors the behaviour of `Neo.SmartContract.Native.RoleManagement` by
//! persisting designated nodes per role and enforcing committee authorization.

use crate::error::{CoreError as Error, CoreResult as Result};
use crate::hardfork::Hardfork;
use crate::impl_native_contract;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::manifest::ContractEventDescriptor;
use crate::smart_contract::native::{LedgerContract, NativeContract, NativeMethod, Role};
use crate::vm_runtime::StackItem;
use crate::UInt160;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

mod metadata;
mod storage;

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

        Self {
            id: Self::ID,
            hash,
            methods: Self::native_methods(),
        }
    }

    fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        self.dispatch_method(engine, method, args)
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

        let public_keys = self.parse_public_keys(&args[1])?;
        if public_keys.is_empty() || public_keys.len() > Self::MAX_NODES {
            return Err(Error::native_contract(format!(
                "Nodes count {} must be between 1 and {}",
                public_keys.len(),
                Self::MAX_NODES
            )));
        }

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

        let mut stored_public_keys = public_keys.clone();
        stored_public_keys.sort();
        if stored_public_keys.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(Error::invalid_operation(
                "Duplicate publickeys are not allowed".to_string(),
            ));
        }

        let serialized_keys = self.serialize_public_keys(&stored_public_keys)?;
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
}

impl NativeContract for RoleManagement {
    impl_native_contract!(hash, "RoleManagement", methods);

    fn id(&self) -> i32 {
        self.id
    }

    fn events(
        &self,
        settings: &crate::protocol_settings::ProtocolSettings,
        block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        Self::event_descriptors(settings, block_height)
    }

    fn activations(&self) -> Vec<Hardfork> {
        vec![Hardfork::HfEchidna]
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
    use crate::hardfork::HardforkManager;
    use crate::ledger::{Block, BlockHeader};
    use crate::network::p2p::payloads::{signer::Signer, transaction::Transaction};
    use crate::persistence::DataCache;
    use crate::protocol_settings::ProtocolSettings;
    use crate::script_builder::ScriptBuilder;
    use crate::smart_contract::binary_serializer::BinarySerializer;
    use crate::smart_contract::call_flags::CallFlags;
    use crate::smart_contract::storage_key::StorageKey;
    use crate::smart_contract::trigger_type::TriggerType;
    use crate::smart_contract::{native::NativeHelpers, StorageItem};
    use crate::vm_runtime::StackItem;
    use crate::witness::Witness;
    use crate::{ECCurve, ECPoint, UInt256, Verifiable, WitnessScope};
    use neo_vm_rs::OpCode;
    use neo_vm_rs::StackValue;
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

    const TEST_GAS_LIMIT: i64 = 400_000_000;

    fn settings_all_active() -> ProtocolSettings {
        let mut settings = ProtocolSettings::default();
        let mut hardforks = std::collections::HashMap::new();
        for hardfork in HardforkManager::all() {
            hardforks.insert(hardfork, 0);
        }
        settings.hardforks = hardforks;
        settings
    }

    fn make_block(index: u32, timestamp: u64) -> Block {
        let header = BlockHeader::new(
            0,
            UInt256::zero(),
            UInt256::zero(),
            timestamp,
            0,
            index,
            0,
            UInt160::zero(),
            vec![Witness::empty()],
        );
        Block::new(header, Vec::new())
    }

    fn make_engine(snapshot: Arc<DataCache>) -> ApplicationEngine {
        let mut container = Transaction::new();
        container.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
        container.add_witness(Witness::new());
        let script_container: Arc<dyn Verifiable> = Arc::new(container);

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

    fn make_engine_with_signers(
        snapshot: Arc<DataCache>,
        settings: ProtocolSettings,
        signers: Vec<Signer>,
        persisting_block: Option<Block>,
    ) -> ApplicationEngine {
        let mut tx = Transaction::new();
        tx.set_signers(signers);
        tx.set_witnesses(vec![Witness::empty(); tx.signers().len()]);
        tx.set_script(vec![OpCode::RET.byte()]);

        let script_container: Arc<dyn Verifiable> = Arc::new(tx);
        ApplicationEngine::new(
            TriggerType::Application,
            Some(script_container),
            snapshot,
            persisting_block,
            settings,
            TEST_GAS_LIMIT,
            None,
        )
        .expect("engine")
    }

    fn committee_address(settings: &ProtocolSettings, snapshot: &DataCache) -> UInt160 {
        NativeHelpers::committee_address(settings, Some(snapshot))
    }

    fn emit_contract_call(
        sb: &mut ScriptBuilder,
        contract_hash: UInt160,
        method: &str,
        mut args: Vec<StackItem>,
    ) {
        let arg_count = args.len();
        for arg in args.drain(..).rev() {
            let value = neo_vm_rs::StackValue::try_from(arg).expect("convert arg");
            sb.emit_push_stack_value(&value).expect("emit arg");
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
    fn dispatch_method_covers_declared_metadata_names() {
        let contract = RoleManagement::new();
        let mut engine = make_engine(Arc::new(DataCache::new(false)));
        let mut names = std::collections::BTreeSet::new();

        for method in contract.methods() {
            if !names.insert(method.name.clone()) {
                continue;
            }

            if let Err(err) = contract.dispatch_method(&mut engine, &method.name, &[]) {
                assert!(
                    !err.to_string().contains("Unknown method:"),
                    "declared method {} did not dispatch: {err}",
                    method.name
                );
            }
        }

        let err = contract
            .dispatch_method(&mut engine, "__missing__", &[])
            .expect_err("unknown method");
        assert!(
            err.to_string().contains("Unknown method: __missing__"),
            "unexpected error: {err}"
        );
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
    fn role_key_suffix_is_role_plus_big_endian_index() {
        assert_eq!(
            RoleManagement::role_key_suffix(Role::Oracle, 0x0102_0304),
            vec![Role::Oracle as u8, 0x01, 0x02, 0x03, 0x04]
        );
    }

    #[test]
    fn serialize_public_keys_uses_stack_value_array_payload() {
        let contract = RoleManagement::new();
        let keys = vec![sample_point(0xAA), sample_point(0xBB)];
        let encoded = contract.serialize_public_keys(&keys).unwrap();
        let value = BinarySerializer::deserialize_stack_value(&encoded).unwrap();

        let StackValue::Array(items) = value else {
            panic!("RoleManagement public keys must serialize as StackValue::Array");
        };
        assert_eq!(items.len(), keys.len());
        for (item, key) in items.iter().zip(keys.iter()) {
            let bytes = item
                .to_byte_string_bytes()
                .expect("public key item should be ByteString-compatible");
            assert_eq!(bytes.len(), 33);
            assert_eq!(bytes, key.to_bytes());
        }
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

    #[test]
    fn designate_as_role_rejects_duplicate_public_keys() {
        let settings = settings_all_active();
        let snapshot = Arc::new(DataCache::new(false));
        let contract = RoleManagement::new();
        let committee = committee_address(&settings, snapshot.as_ref());
        let persisting_block = make_block(1000, 1_000);
        let duplicate_key = sample_point(0x42);
        let args = vec![
            StackItem::from_int(Role::Oracle as u8 as i64)
                .as_bytes()
                .expect("role bytes"),
            contract
                .serialize_public_keys(&[duplicate_key.clone(), duplicate_key])
                .expect("public keys payload"),
        ];

        let mut engine = make_engine_with_signers(
            Arc::clone(&snapshot),
            settings,
            vec![Signer::new(committee, WitnessScope::GLOBAL)],
            Some(persisting_block),
        );

        let err = engine
            .call_native_contract(contract.hash(), "designateAsRole", &args)
            .expect_err("duplicate public keys should be rejected");
        assert!(
            err.to_string()
                .contains("Duplicate publickeys are not allowed"),
            "unexpected error: {err}"
        );

        let key = StorageKey::new(
            RoleManagement::ID,
            RoleManagement::role_key_suffix(Role::Oracle, 1001),
        );
        assert!(
            snapshot.get(&key).is_none(),
            "duplicate designation must not be stored"
        );
    }

    #[test]
    fn designate_as_role_reports_existing_designation_before_duplicate_validation() {
        let settings = settings_all_active();
        let snapshot = Arc::new(DataCache::new(false));
        let contract = RoleManagement::new();
        let committee = committee_address(&settings, snapshot.as_ref());
        let persisting_block = make_block(1000, 1_000);
        let designation_key = StorageKey::new(
            RoleManagement::ID,
            RoleManagement::role_key_suffix(Role::Oracle, 1001),
        );
        snapshot.add(
            designation_key,
            StorageItem::from_bytes(
                contract
                    .serialize_public_keys(&[sample_point(0x10)])
                    .expect("existing designation payload"),
            ),
        );

        let duplicate_key = sample_point(0x43);
        let args = vec![
            StackItem::from_int(Role::Oracle as u8 as i64)
                .as_bytes()
                .expect("role bytes"),
            contract
                .serialize_public_keys(&[duplicate_key.clone(), duplicate_key])
                .expect("public keys payload"),
        ];

        let mut engine = make_engine_with_signers(
            Arc::clone(&snapshot),
            settings,
            vec![Signer::new(committee, WitnessScope::GLOBAL)],
            Some(persisting_block),
        );

        let err = engine
            .call_native_contract(contract.hash(), "designateAsRole", &args)
            .expect_err("existing designation should fail before duplicate validation");
        assert!(
            err.to_string().contains("Role already designated"),
            "unexpected error: {err}"
        );
        assert!(
            !err.to_string()
                .contains("Duplicate publickeys are not allowed"),
            "duplicate validation should not run before existing designation check: {err}"
        );
    }
}
