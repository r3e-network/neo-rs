use super::*;

#[tokio::test]
async fn initialize_bootstraps_genesis_once_and_inventory_runs_native_hooks() {
    let (service, _handle, snapshot, state_store) = store_fixture_with_state_service();

    // C# Blockchain.OnInitialize: an uninitialized store gets the
    // genesis block persisted (native deploy seeds + mints).
    service.initialize().await;
    assert!(crate::native_persist::chain_state_initialized(&snapshot));
    assert_eq!(
        neo_total_supply(&snapshot),
        Some(num_bigint::BigInt::from(100_000_000)),
        "genesis minted the NEO total supply"
    );
    assert!(
        service.ledger.block_hash_at(0).is_some(),
        "genesis cached in the ledger"
    );
    assert!(
        state_store
            .mpt()
            .expect("state store exposes MPT")
            .get_state_root(0)
            .is_some(),
        "genesis writes the local state-root record for block 0"
    );

    // Re-initializing must NOT re-persist (the initialized probe
    // guards the C# `Ledger.Initialized` branch): the supply stays
    // 100M instead of doubling.
    service.initialize().await;
    assert_eq!(
        neo_total_supply(&snapshot),
        Some(num_bigint::BigInt::from(100_000_000))
    );

    // An inventory block at the next height runs the OnPersist /
    // PostPersist native hooks over the same store: block 1 mints
    // the 0.5-GAS committee reward to standby_committee[1 % 21].
    // The synthetic header carries no real consensus witness, so it goes
    // through the pre-verified path (the consensus-driver submission route);
    // witness verification of peer-relayed blocks has its own tests below.
    let mut header = Header::new();
    header.set_index(1);
    let block = Arc::new(Block::from_parts(header, vec![]));
    service
        .handle_block_inventory(block, false, true)
        .await
        .expect("inventory block persists");
    assert_eq!(service.ledger.current_height(), 1);

    let settings = neo_config::ProtocolSettings::default();
    let member = &settings.standby_committee[1];
    let script = neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
        &member.to_bytes(),
    );
    let account = neo_primitives::UInt160::from_script(&script);
    let mut key = vec![20u8]; // shared NEP-17 Prefix_Account
    key.extend_from_slice(&account.to_bytes());
    assert!(
        snapshot
            .get(&neo_storage::StorageKey::new(
                neo_native_contracts::GasToken::ID,
                key
            ))
            .is_some(),
        "block-1 PostPersist minted the rotating committee reward"
    );
}

#[tokio::test]
async fn future_inventory_block_is_parked_then_drained_after_parent_persists() {
    let (service, _handle, snapshot) = store_fixture();
    service.initialize().await;

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Arc::new(Block::from_parts(header1, vec![]));
    let block1_hash = BlockchainService::try_block_hash(block1.as_ref()).expect("block1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());
    let block2 = Arc::new(Block::from_parts(header2, vec![]));

    service
        .handle_block_inventory(Arc::clone(&block2), false, true)
        .await
        .expect("future block is parked, not rejected");
    assert_eq!(service.ledger.current_height(), 0);
    assert_eq!(service.unverified_block_count(), 1);
    assert!(service.ledger.block_hash_at(2).is_none());

    service
        .handle_block_inventory(block1, false, true)
        .await
        .expect("parent block persists and drains child");

    assert_eq!(service.ledger.current_height(), 2);
    assert_eq!(service.unverified_block_count(), 0);
    assert!(service.ledger.block_hash_at(1).is_some());
    assert!(service.ledger.block_hash_at(2).is_some());
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        2
    );
}

#[tokio::test]
async fn stop_height_allows_target_block_and_rejects_later_blocks() {
    let (mut service, _handle, _snapshot) = store_fixture();
    service.set_stop_at_height(Some(1));
    service.initialize().await;

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Arc::new(Block::from_parts(header1, vec![]));
    let block1_hash = BlockchainService::try_block_hash(block1.as_ref()).expect("block1 hash");

    service
        .handle_block_inventory(block1, false, true)
        .await
        .expect("target stop-height block persists");
    assert_eq!(service.ledger.current_height(), 1);

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());
    let err = service
        .handle_block_inventory(Arc::new(Block::from_parts(header2, vec![])), false, true)
        .await
        .expect_err("block after stop height must not persist");
    assert!(
        err.to_string().contains("stop height 1"),
        "error should name the configured stop height: {err}"
    );
    assert_eq!(service.ledger.current_height(), 1);
    assert!(service.ledger.block_hash_at(2).is_none());
}

#[tokio::test]
async fn future_block_with_cached_header_hash_mismatch_is_rejected_not_parked() {
    let (service, _handle) = fixture();
    let mut header1 = Header::new();
    header1.set_index(1);
    let mut header2 = Header::new();
    header2.set_index(2);
    service.handle_headers(vec![header1, header2.clone()]);
    assert_eq!(service.header_cache.count(), 2, "headers cached first");

    let mut competing = header2;
    competing.set_nonce(0xAA55_AA55_AA55_AA55);
    let err = service
        .handle_block_inventory(Arc::new(Block::from_parts(competing, vec![])), false, true)
        .await
        .expect_err("future block hash must match an existing cached header");
    assert!(
        err.to_string().contains("cached header"),
        "rejection should name cached header mismatch: {err}"
    );
    assert_eq!(
        service.unverified_block_count(),
        0,
        "mismatched future block must not be parked"
    );
}

#[tokio::test]
async fn import_verify_true_rejects_invalid_header_like_csharp() {
    let (service, _handle, snapshot) = store_fixture();
    service.initialize().await;

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp());
    header.set_next_consensus(*genesis.header.next_consensus());

    service
        .handle_import(Import {
            blocks: vec![Block::from_parts(header, vec![])],
            verify: true,
        })
        .await;

    assert_eq!(
        service.ledger.current_height(),
        0,
        "C# OnImport(verify: true) stops before persisting an invalid header"
    );
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        0
    );
}

#[tokio::test]
async fn import_verify_false_skips_header_verification_like_csharp() {
    let (service, _handle, snapshot) = store_fixture();
    service.initialize().await;

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp());
    header.set_next_consensus(*genesis.header.next_consensus());
    assert!(
        service.header_cache.add(header.clone()),
        "test starts with a header-first cached block"
    );

    service
        .handle_import(Import {
            blocks: vec![Block::from_parts(header, vec![])],
            verify: false,
        })
        .await;

    assert_eq!(
        service.ledger.current_height(),
        1,
        "C# OnImport(verify: false) bypasses Block.Verify and persists the next block"
    );
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        1
    );
    assert_eq!(
        service.header_cache.count(),
        0,
        "C# Persist removes the first cached header after committing the block"
    );
}

#[tokio::test]
async fn persisted_inventory_block_removes_cached_header_after_mempool_update() {
    neo_native_contracts::install();
    let settings = neo_config::ProtocolSettings::default();
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let system: Arc<dyn SystemContext> = Arc::new(StoreContext {
        snapshot: Arc::clone(&snapshot),
        settings: Arc::new(settings.clone()),
        state_service: None,
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let reverify_calls = Arc::new(AtomicUsize::new(0));
    let mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>> =
        Arc::new(Mutex::new(RecordingMempool {
            reverify_calls: Arc::clone(&reverify_calls),
        }));
    let (service, _handle) =
        BlockchainService::with_defaults(system, ledger, Arc::clone(&header_cache), mempool);

    service.initialize().await;
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_next_consensus(*genesis.header.next_consensus());
    assert!(header_cache.add(header.clone()));

    service
        .handle_block_inventory(Arc::new(Block::from_parts(header, vec![])), false, true)
        .await
        .expect("cached-header block persists");

    assert_eq!(service.ledger.current_height(), 1);
    assert_eq!(
        reverify_calls.load(Ordering::SeqCst),
        0,
        "C# MemoryPool.UpdatePoolForBlockPersisted skips reverify while future headers are still cached"
    );
    assert_eq!(
        service.header_cache.count(),
        0,
        "C# Blockchain.Persist removes the consumed header after mempool update"
    );
}

/// End-to-end consensus-witness verification of a peer-relayed block: a
/// block signed by the network's validator (1-of-1 multisig over the C#
/// sign data = network magic LE + header hash) is accepted, and the same
/// block with a tampered signature is rejected. Proves the whole
/// `Header.Verify` path (prev-block lookup, timestamp/primary checks,
/// script-hash match against prev `NextConsensus`, CheckMultisig over the
/// header sign data) so live sync cannot be stalled by a broken verifier.
#[tokio::test]
async fn peer_block_witness_verification_accepts_valid_and_rejects_tampered() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;
    let network = settings.network;

    let (service, _handle, _snapshot) = store_fixture_with(settings.clone());
    service.initialize().await;
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    // Block 1 over genesis (no transactions; merkle root stays zero).
    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_primary_index(0);
    header.set_next_consensus(*genesis.header.next_consensus());

    // C# sign data: network magic (LE) + header hash (witness excluded).
    let mut sign_data = Vec::with_capacity(36);
    sign_data.extend_from_slice(&network.to_le_bytes());
    sign_data.extend_from_slice(&header.hash().to_bytes());
    let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");
    let invocation = |sig: &[u8]| {
        let mut script = vec![0x0C, 64]; // PUSHDATA1 64
        script.extend_from_slice(sig);
        script
    };

    // Tampered signature -> rejected, nothing persisted.
    let mut tampered_sig = signature;
    tampered_sig[10] ^= 0xFF;
    let mut tampered = header.clone();
    tampered.witness =
        neo_payloads::Witness::new_with_scripts(invocation(&tampered_sig), verification.clone());
    let err = service
        .handle_block_inventory(Arc::new(Block::from_parts(tampered, vec![])), false, false)
        .await
        .expect_err("tampered consensus witness must be rejected");
    assert!(
        err.to_string().contains("witness"),
        "rejection names the witness: {err}"
    );
    assert_eq!(service.ledger.current_height(), 0);

    // Valid signature -> accepted and persisted.
    header.witness = neo_payloads::Witness::new_with_scripts(invocation(&signature), verification);
    service
        .handle_block_inventory(Arc::new(Block::from_parts(header, vec![])), false, false)
        .await
        .expect("validly signed peer block is accepted");
    assert_eq!(service.ledger.current_height(), 1);
}

/// C# `Blockchain.OnNewBlock` rejects a full block whose height is already
/// represented in `HeaderCache` unless its hash equals the cached header
/// (`Blockchain.cs:241-243`).
#[tokio::test]
async fn peer_block_must_match_cached_header_hash() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;
    let network = settings.network;

    let (service, _handle, _snapshot) = store_fixture_with(settings.clone());
    service.initialize().await;
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");

    let sign_header = |header: &Header| {
        let mut sign_data = Vec::with_capacity(36);
        sign_data.extend_from_slice(&network.to_le_bytes());
        sign_data.extend_from_slice(&header.hash().to_bytes());
        let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
        let mut invocation = vec![0x0C, 64];
        invocation.extend_from_slice(&signature);
        neo_payloads::Witness::new_with_scripts(invocation, verification.clone())
    };

    let mut cached = Header::new();
    cached.set_index(1);
    cached.set_prev_hash(genesis.hash());
    cached.set_timestamp(genesis.header.timestamp() + 15_000);
    cached.set_primary_index(0);
    cached.set_next_consensus(*genesis.header.next_consensus());
    cached.witness = sign_header(&cached);
    service.handle_headers(vec![cached.clone()]);
    assert_eq!(service.header_cache.count(), 1, "header cached first");

    let mut competing = cached;
    competing.set_nonce(0xAA55_AA55_AA55_AA55);
    competing.witness = sign_header(&competing);

    let err = service
        .handle_block_inventory(Arc::new(Block::from_parts(competing, vec![])), false, false)
        .await
        .expect_err("block hash must match the cached header at the same height");
    assert!(
        err.to_string().contains("cached header"),
        "rejection should name cached header mismatch: {err}"
    );
    assert_eq!(
        service.ledger.current_height(),
        0,
        "mismatched block must not be persisted"
    );
}

/// Public `BlockchainHandle::import_block` is the RPC/user-submitted block
/// path, so it must wait for the service verdict and verify the consensus
/// witness instead of reporting success after merely queueing the command.
#[tokio::test]
async fn handle_import_block_reports_rejection_and_verifies_witness() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;
    let network = settings.network;

    let (service, handle, _snapshot) = store_fixture_with(settings.clone());
    service.initialize().await;
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_primary_index(0);
    header.set_next_consensus(*genesis.header.next_consensus());

    let mut sign_data = Vec::with_capacity(36);
    sign_data.extend_from_slice(&network.to_le_bytes());
    sign_data.extend_from_slice(&header.hash().to_bytes());
    let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");
    let invocation = |sig: &[u8]| {
        let mut script = vec![0x0C, 64];
        script.extend_from_slice(sig);
        script
    };

    let mut tampered_signature = signature;
    tampered_signature[10] ^= 0xFF;
    let mut tampered_header = header.clone();
    tampered_header.witness = neo_payloads::Witness::new_with_scripts(
        invocation(&tampered_signature),
        verification.clone(),
    );

    header.witness = neo_payloads::Witness::new_with_scripts(invocation(&signature), verification);

    let runner = tokio::spawn(service.run());

    let rejected = handle
        .import_block(Block::from_parts(tampered_header, vec![]))
        .await
        .expect("import command reply");
    assert!(
        !rejected,
        "tampered witness must not be reported as imported"
    );
    assert_eq!(handle.get_height().await.expect("height reply"), 0);

    let imported = handle
        .import_block(Block::from_parts(header, vec![]))
        .await
        .expect("import command reply");
    assert!(imported, "validly signed block advances the tip");
    assert_eq!(handle.get_height().await.expect("height reply"), 1);

    drop(handle);
    runner
        .await
        .expect("service exits after command channel closes");
}
