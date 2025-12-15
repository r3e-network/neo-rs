//
// tests.rs - Unit tests for Blockchain actor
//

use super::*;

mod tests {
    use super::{
        classify_import_block, should_schedule_reverify_idle, Blockchain, ImportDisposition,
        StateRoot, VerifyResult, STATE_SERVICE_CATEGORY,
    };
    use crate::neo_io::BinaryWriter;
    use crate::network::p2p::payloads::extensible_payload::ExtensiblePayload;
    use crate::network::p2p::payloads::witness::Witness as PayloadWitness;
    use crate::persistence::StoreCache;
    use crate::smart_contract::Contract;
    use crate::smart_contract::native::{role_management::RoleManagement, NativeContract, Role};
    use crate::smart_contract::{StorageItem, StorageKey};
    use crate::wallets::KeyPair;
    use crate::{neo_io::Serializable, NeoSystem, ProtocolSettings};
    use crate::state_service::state_store::StateServiceSettings;
    use neo_vm::op_code::OpCode;
    use tokio::time::{sleep, timeout, Duration};

    fn sign_extensible_payload(
        payload: &mut ExtensiblePayload,
        keypair: &KeyPair,
        settings: &ProtocolSettings,
    ) {
        let mut payload_for_hash = payload.clone();
        let hash = payload_for_hash.hash();
        let mut sign_data = Vec::with_capacity(4 + 32);
        sign_data.extend_from_slice(&settings.network.to_le_bytes());
        sign_data.extend_from_slice(&hash.to_array());
        let signature = keypair.sign(&sign_data).expect("sign payload");

        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(OpCode::PUSHDATA1 as u8);
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);
        payload.witness = PayloadWitness::new_with_scripts(invocation, keypair.get_verification_script());
    }

    #[test]
    fn classify_import_block_returns_already_seen_for_past_height() {
        assert_eq!(classify_import_block(10, 5), ImportDisposition::AlreadySeen);
        assert_eq!(
            classify_import_block(10, 10),
            ImportDisposition::AlreadySeen
        );
    }

    #[test]
    fn classify_import_block_returns_next_expected_when_in_sequence() {
        assert_eq!(classify_import_block(7, 8), ImportDisposition::NextExpected);
    }

    #[test]
    fn classify_import_block_detects_future_gap() {
        assert_eq!(classify_import_block(3, 8), ImportDisposition::FutureGap);
    }

    #[test]
    fn schedule_idle_only_when_more_pending_without_backlog() {
        assert!(should_schedule_reverify_idle(true, false));
        assert!(!should_schedule_reverify_idle(false, false));
        assert!(!should_schedule_reverify_idle(true, true));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn state_service_payload_ingests_into_shared_state_store() {
        let mut settings = ProtocolSettings::mainnet();
        let keypair = KeyPair::generate().expect("generate keypair");
        let validator = keypair
            .get_public_key_point()
            .expect("public key point from keypair");
        settings.standby_committee = vec![validator.clone()];
        settings.validators_count = 1;
        settings.network = 0x42_4242;

        let system = NeoSystem::new_with_state_service(
            settings.clone(),
            None,
            None,
            Some(StateServiceSettings::default()),
        )
        .expect("NeoSystem::new_with_state_service should succeed");
        let state_store = system
            .state_store()
            .expect("state store lookup")
            .expect("state store registered");

        let genesis_height = 0;
        timeout(Duration::from_secs(2), async {
            loop {
                if state_store.local_root_index() == Some(genesis_height) {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("genesis state root should be computed");

        let height: u32 = 5;

        // Seed a RoleManagement designation for state validators so witness verification matches
        // Neo.Plugins.StateService rules.
        let mut store_cache = StoreCache::new_from_store(system.store(), false);
        let mut suffix = vec![Role::StateValidator as u8];
        suffix.extend_from_slice(&height.to_be_bytes());
        let key = StorageKey::new(RoleManagement::new().id(), suffix);
        let mut value = Vec::with_capacity(4 + 33);
        value.extend_from_slice(&1u32.to_le_bytes());
        value.extend_from_slice(&validator.encode_compressed().expect("compress validator"));
        store_cache
            .data_cache()
            .add(key, StorageItem::from_bytes(value));
        store_cache.commit();

        // Advance the local state root index to a height covered by the designation.
        state_store.update_local_state_root_snapshot(height, std::iter::empty());
        state_store.update_local_state_root(height);
        let root_hash = state_store
            .current_local_root_hash()
            .expect("local root hash seeded");

        let mut state_root = StateRoot::new_current(height, root_hash);
        let hash = state_root.hash();
        let mut sign_data = Vec::with_capacity(4 + hash.to_bytes().len());
        sign_data.extend_from_slice(&settings.network.to_le_bytes());
        sign_data.extend_from_slice(&hash.to_array());
        let signature = keypair.sign(&sign_data).expect("sign state root");
        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(OpCode::PUSHDATA1 as u8);
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);
        let verification_script = Contract::create_multi_sig_redeem_script(1, &[validator]);
        state_root.witness = Some(PayloadWitness::new_with_scripts(
            invocation,
            verification_script,
        ));

        let mut writer = BinaryWriter::new();
        state_root
            .serialize(&mut writer)
            .expect("serialize state root");
        let mut payload_bytes = vec![0u8];
        payload_bytes.extend_from_slice(&writer.into_bytes());

        let mut payload = ExtensiblePayload::new();
        payload.category = STATE_SERVICE_CATEGORY.to_string();
        payload.valid_block_start = 0;
        payload.valid_block_end = height + 10;
        payload.sender = keypair.get_script_hash();
        payload.data = payload_bytes;

        let blockchain = Blockchain::new(system.ledger_context());
        let accepted = blockchain
            .process_state_service_payload(&system.context(), &payload)
            .expect("state service payload");
        assert!(accepted);
        assert_eq!(state_store.validated_root_index(), Some(height));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn extensible_payload_requires_whitelisted_sender() {
        let mut settings = ProtocolSettings::mainnet();
        settings.network = 0x1122_3344;

        // Single validator/committee entry.
        let validator_keypair = KeyPair::generate().expect("generate validator keypair");
        let validator_pub = validator_keypair
            .get_public_key_point()
            .expect("validator public key");
        settings.standby_committee = vec![validator_pub];
        settings.validators_count = 1;

        let system = NeoSystem::new(settings.clone(), None, None).expect("NeoSystem::new");
        let mut blockchain = Blockchain::new(system.ledger_context());
        blockchain.system_context = Some(system.context());

        // Sender that is not in committee/validators/state validators.
        let attacker = KeyPair::generate().expect("generate attacker keypair");

        let mut payload = ExtensiblePayload::new();
        payload.category = "test".to_string();
        payload.valid_block_start = 0;
        payload.valid_block_end = 10;
        payload.sender = attacker.get_script_hash();
        payload.data = vec![1, 2, 3];
        sign_extensible_payload(&mut payload, &attacker, &settings);

        let result = blockchain.on_new_extensible(payload).await;
        assert_eq!(result, VerifyResult::Invalid);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn extensible_payload_accepts_validator_sender() {
        let mut settings = ProtocolSettings::mainnet();
        settings.network = 0x5566_7788;

        let validator_keypair = KeyPair::generate().expect("generate validator keypair");
        let validator_pub = validator_keypair
            .get_public_key_point()
            .expect("validator public key");
        settings.standby_committee = vec![validator_pub];
        settings.validators_count = 1;

        let system = NeoSystem::new(settings.clone(), None, None).expect("NeoSystem::new");
        let mut blockchain = Blockchain::new(system.ledger_context());
        blockchain.system_context = Some(system.context());

        let mut payload = ExtensiblePayload::new();
        payload.category = "test".to_string();
        payload.valid_block_start = 0;
        payload.valid_block_end = 10;
        payload.sender = validator_keypair.get_script_hash();
        payload.data = vec![9, 8, 7];
        sign_extensible_payload(&mut payload, &validator_keypair, &settings);

        let result = blockchain.on_new_extensible(payload).await;
        assert_eq!(result, VerifyResult::Succeed);
    }
}
