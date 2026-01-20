//
// tests.rs - Unit tests for Blockchain actor
//

#![allow(clippy::module_inception)]

use super::*;

mod tests {
    use super::{
        classify_import_block, should_schedule_reverify_idle, Blockchain, ImportDisposition,
        StateRoot, VerifyResult, STATE_SERVICE_CATEGORY,
    };
    use crate::neo_io::BinaryWriter;
    use crate::network::p2p::payloads::extensible_payload::ExtensiblePayload;
    use crate::network::p2p::payloads::witness::Witness as PayloadWitness;
    use crate::network::p2p::{
        helper::get_sign_data_vec,
        payloads::{
            block::Block as PayloadBlock, conflicts::Conflicts, header::Header, signer::Signer,
            transaction::Transaction, transaction_attribute::TransactionAttribute,
            witness::Witness, InventoryType,
        },
    };
    use crate::persistence::StoreCache;
    use crate::smart_contract::binary_serializer::BinarySerializer;
    use crate::smart_contract::native::fungible_token::PREFIX_ACCOUNT;
    use crate::smart_contract::native::gas_token::GasToken;
    use crate::smart_contract::native::{
        role_management::RoleManagement, AccountState, NativeContract, Role,
    };
    use crate::smart_contract::Contract;
    use crate::smart_contract::{IInteroperable, StorageItem, StorageKey};
    use crate::state_service::state_store::StateServiceSettings;
    use crate::wallets::KeyPair;
    use crate::WitnessScope;
    use crate::{neo_io::Serializable, NeoSystem, ProtocolSettings, UInt160, UInt256};
    use neo_vm::execution_engine_limits::ExecutionEngineLimits;
    use neo_vm::op_code::OpCode;
    use num_bigint::BigInt;
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
        payload.witness =
            PayloadWitness::new_with_scripts(invocation, keypair.get_verification_script());
    }

    fn build_signed_transaction(settings: &ProtocolSettings, keypair: &KeyPair) -> Transaction {
        build_signed_transaction_with_attrs(settings, keypair, 1, Vec::new())
    }

    fn build_signed_transaction_with_attrs(
        settings: &ProtocolSettings,
        keypair: &KeyPair,
        valid_until_block: u32,
        attributes: Vec<TransactionAttribute>,
    ) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_network_fee(1_0000_0000);
        tx.set_system_fee(30);
        tx.set_valid_until_block(valid_until_block);
        tx.set_script(vec![OpCode::PUSH1 as u8]);
        tx.set_signers(vec![Signer::new(
            keypair.get_script_hash(),
            WitnessScope::GLOBAL,
        )]);
        tx.set_attributes(attributes);

        let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
        let signature = keypair.sign(&sign_data).expect("sign");
        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(OpCode::PUSHDATA1 as u8);
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);
        tx.set_witnesses(vec![Witness::new_with_scripts(
            invocation,
            keypair.get_verification_script(),
        )]);
        tx
    }

    fn seed_gas_balance(store: &mut StoreCache, account: UInt160, amount: i64) {
        let key = StorageKey::create_with_uint160(GasToken::new().id(), PREFIX_ACCOUNT, &account);
        let state = AccountState::with_balance(BigInt::from(amount));
        let bytes =
            BinarySerializer::serialize(&state.to_stack_item(), &ExecutionEngineLimits::default())
                .expect("serialize account state");
        store
            .data_cache()
            .update(key, StorageItem::from_bytes(bytes));
        store.commit();
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
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new("state=debug"))
            .with_test_writer()
            .try_init();

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
        let value = RoleManagement::new()
            .serialize_public_keys(&[validator.clone()])
            .expect("serialize public keys");
        store_cache
            .data_cache()
            .add(key, StorageItem::from_bytes(value));
        store_cache.commit();

        // Sanity check: RoleManagement designation should be visible through the verifier
        // snapshot provider used by StateRootVerifier::from_store.
        let verifier_snapshot = StoreCache::new_from_store(system.store(), true)
            .data_cache()
            .clone_cache();
        let designated = RoleManagement::new()
            .get_designated_by_role_at(&verifier_snapshot, Role::StateValidator, height)
            .expect("load designated validators");
        assert_eq!(designated.len(), 1);

        // Advance the local state root index to a height covered by the designation.
        state_store.update_local_state_root_snapshot(height, std::iter::empty());
        state_store.update_local_state_root(height);
        assert_eq!(
            state_store.local_root_index(),
            Some(height),
            "local state root index should advance after committing snapshot"
        );
        let local_root = state_store
            .get_state_root(height)
            .expect("local state root should be persisted for ingest");
        assert!(
            local_root.witness.is_none(),
            "local state root should not be pre-validated"
        );
        let root_hash = state_store
            .current_local_root_hash()
            .expect("local root hash seeded");
        assert_eq!(
            local_root.root_hash, root_hash,
            "local root hash should match persisted state root"
        );

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
        assert!(
            state_root.verify(&settings, &verifier_snapshot),
            "state root witness should verify against designated state validators"
        );

        let mut writer = BinaryWriter::new();
        state_root
            .serialize(&mut writer)
            .expect("serialize state root");
        // Neo.Plugins.StateService.Network.MessageType: StateRoot = 1.
        let mut payload_bytes = vec![1u8];
        payload_bytes.extend_from_slice(&writer.into_bytes());

        let mut payload = ExtensiblePayload::new();
        payload.category = STATE_SERVICE_CATEGORY.to_string();
        payload.valid_block_start = 0;
        payload.valid_block_end = height + 10;
        payload.sender = keypair.get_script_hash();
        payload.data = payload_bytes;
        sign_extensible_payload(&mut payload, &keypair, &settings);

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

    #[tokio::test(flavor = "multi_thread")]
    async fn reverify_accepts_deserialized_payload() {
        use crate::ledger::blockchain::{
            BlockchainCommand, InventoryPayload, Reverify, ReverifyItem,
        };

        let settings = ProtocolSettings::mainnet();
        let system = NeoSystem::new(settings.clone(), None, None).expect("NeoSystem::new");

        // We'll send a Reverify command with a Block payload.
        // Using genesis block for simplicity.
        let genesis = system.genesis_block();
        let item = ReverifyItem {
            payload: InventoryPayload::Block(Box::new(genesis.as_ref().clone())),
            block_index: Some(0),
        };
        let reverify = Reverify {
            inventories: vec![item],
        };

        // Send to blockchain actor
        let blockchain = system.blockchain_actor();
        blockchain
            .tell(BlockchainCommand::Reverify(reverify))
            .expect("send failed");

        // Wait a bit for processing to ensure no crashes
        sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn relay_accepts_valid_transaction_then_reports_already_in_pool() {
        let settings = ProtocolSettings::default();
        let system = NeoSystem::new(settings.clone(), None, None).expect("neo system");
        let keypair = KeyPair::generate().expect("keypair");
        let account = keypair.get_script_hash();

        let mut store_cache = StoreCache::new_from_store(system.store(), false);
        seed_gas_balance(&mut store_cache, account, 50_0000_0000);

        let mut blockchain = Blockchain::new(system.ledger_context());
        blockchain.system_context = Some(system.context());

        let tx = build_signed_transaction(&settings, &keypair);
        assert_eq!(blockchain.on_new_transaction(&tx), VerifyResult::Succeed);
        assert_eq!(
            blockchain.on_new_transaction(&tx),
            VerifyResult::AlreadyInPool
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn relay_rejects_mismatched_signers_and_witnesses() {
        let settings = ProtocolSettings::default();
        let system = NeoSystem::new(settings.clone(), None, None).expect("neo system");
        let keypair = KeyPair::generate().expect("keypair");

        let mut blockchain = Blockchain::new(system.ledger_context());
        blockchain.system_context = Some(system.context());

        let mut tx = build_signed_transaction(&settings, &keypair);
        tx.set_signers(Vec::new());

        assert_eq!(blockchain.on_new_transaction(&tx), VerifyResult::Invalid);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn relay_rejects_on_chain_conflict_with_same_sender() {
        let settings = ProtocolSettings::default();
        let system = NeoSystem::new(settings.clone(), None, None).expect("neo system");
        let keypair_a = KeyPair::generate().expect("keypair a");
        let keypair_b = KeyPair::generate().expect("keypair b");

        let account_a = keypair_a.get_script_hash();
        let account_b = keypair_b.get_script_hash();

        let mut store_cache = StoreCache::new_from_store(system.store(), false);
        seed_gas_balance(&mut store_cache, account_a, 50_0000_0000);
        seed_gas_balance(&mut store_cache, account_b, 50_0000_0000);

        let tx2 = build_signed_transaction_with_attrs(&settings, &keypair_a, 10, Vec::new());
        let tx3 = build_signed_transaction_with_attrs(&settings, &keypair_b, 10, Vec::new());
        let tx1 = build_signed_transaction_with_attrs(
            &settings,
            &keypair_a,
            10,
            vec![
                TransactionAttribute::Conflicts(Conflicts::new(tx2.hash())),
                TransactionAttribute::Conflicts(Conflicts::new(tx3.hash())),
            ],
        );

        let mut block = PayloadBlock::new();
        let mut header = Header::new();
        header.set_index(5);
        header.set_prev_hash(UInt256::zero());
        header.set_merkle_root(UInt256::zero());
        header.set_next_consensus(UInt160::zero());
        header.set_timestamp(0);
        header.witness = Witness::new();
        block.header = header;
        block.transactions = vec![tx1];

        system.persist_block(block).expect("persist block");

        let mut blockchain = Blockchain::new(system.ledger_context());
        blockchain.system_context = Some(system.context());

        assert_eq!(
            blockchain.on_new_transaction(&tx2),
            VerifyResult::HasConflicts
        );
        assert_eq!(blockchain.on_new_transaction(&tx3), VerifyResult::Succeed);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn import_rejects_out_of_order_blocks() {
        use crate::ledger::blockchain::{BlockchainCommand, Import};

        let settings = ProtocolSettings::mainnet();
        let system = NeoSystem::new(settings.clone(), None, None).expect("NeoSystem::new");
        let blockchain = system.blockchain_actor();

        // Genesis is block 0. Current height is 0.
        // Send block 2 (future gap).
        let mut block = system.genesis_block().as_ref().clone();
        block.header.set_index(2);

        let import = Import {
            blocks: vec![block],
            verify: true,
        };

        blockchain
            .tell(BlockchainCommand::Import(import))
            .expect("send failed");

        sleep(Duration::from_millis(100)).await;

        // Verify height didn't change
        let height = system.ledger_context().current_height();
        assert_eq!(height, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn import_rejects_invalid_block_when_verify_enabled() {
        use crate::ledger::blockchain::{BlockchainCommand, Import};
        use crate::UInt256;

        let settings = ProtocolSettings::mainnet();
        let system = NeoSystem::new(settings.clone(), None, None).expect("NeoSystem::new");
        let blockchain = system.blockchain_actor();

        let mut block = system.genesis_block().as_ref().clone();
        block.header.set_index(1);
        let invalid_prev = UInt256::from_bytes(&[1u8; 32]).expect("construct invalid prev hash");
        block.header.set_prev_hash(invalid_prev);

        let import = Import {
            blocks: vec![block],
            verify: true,
        };

        blockchain
            .tell(BlockchainCommand::Import(import))
            .expect("send failed");

        sleep(Duration::from_millis(100)).await;

        let height = system.ledger_context().current_height();
        assert_eq!(height, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reverify_accepts_raw_transaction_payload() {
        use crate::ledger::blockchain::{
            BlockchainCommand, InventoryPayload, Reverify, ReverifyItem,
        };

        let settings = ProtocolSettings::mainnet();
        let system = NeoSystem::new(settings.clone(), None, None).expect("NeoSystem::new");
        let keypair = KeyPair::generate().expect("keypair");

        let tx = build_signed_transaction(&settings, &keypair);
        let mut writer = BinaryWriter::new();
        tx.serialize(&mut writer).expect("serialize tx");
        let payload = writer.into_bytes();

        // Sanity check: raw payload deserializes before handing to the actor.
        assert!(Blockchain::deserialize_inventory::<Transaction>(&payload).is_some());

        let item = ReverifyItem {
            payload: InventoryPayload::Raw(InventoryType::Transaction, payload),
            block_index: None,
        };
        let reverify = Reverify {
            inventories: vec![item],
        };

        let blockchain = system.blockchain_actor();
        blockchain
            .tell(BlockchainCommand::Reverify(reverify))
            .expect("send failed");

        sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reverify_does_not_schedule_idle_when_header_backlog_exists() {
        use crate::ledger::blockchain::{BlockchainCommand, Reverify};
        use crate::network::p2p::payloads::Header;

        let settings = ProtocolSettings::mainnet();
        let system = NeoSystem::new(settings.clone(), None, None).expect("NeoSystem::new");
        let context = system.context();

        timeout(Duration::from_secs(2), async {
            loop {
                if system.ledger_context().block_hash_at(0).is_some() {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("genesis block should be persisted before reverify test");
        sleep(Duration::from_millis(50)).await;

        // Seed a header backlog so reverify should avoid scheduling idle.
        let mut header = Header::new();
        header.set_index(1);
        context.header_cache().add(header);

        let memory_pool = context.memory_pool_handle();
        memory_pool
            .lock()
            .insert_unverified_for_test(Transaction::new());

        let reverify = Reverify {
            inventories: Vec::new(),
        };

        let blockchain = system.blockchain_actor();
        blockchain
            .tell(BlockchainCommand::Reverify(reverify))
            .expect("send failed");

        sleep(Duration::from_millis(200)).await;

        assert_eq!(memory_pool.lock().unverified_count(), 1);
    }
}
