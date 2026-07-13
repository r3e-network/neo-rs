use super::*;

#[test]
fn gas_account_key_from_address_matches_mainnet_sender_key() {
    let key = gas_account_key_from_address("NUDcRfftT99w4m2puzTxQToHxZPjQ9NN9n").expect("gas key");

    assert_eq!(base64_encode(&key), "FFsXKfRyjg82/UCteNWE46qLP5K8");
}

#[test]
fn storage_key_bytes_prefix_contract_id_little_endian() {
    let bytes = storage_key_bytes(-6, &[0x14, 0xAA, 0xBB]);

    assert_eq!(hex::encode(bytes), "faffffff14aabb");
}

#[test]
fn storage_key_prefix_bytes_uses_contract_id_without_suffix() {
    let bytes = storage_key_prefix_bytes(33);

    assert_eq!(hex::encode(bytes), "21000000");
}

#[test]
fn mpt_state_root_key_uses_state_service_big_endian_height() {
    let key = mpt_state_root_key(474_701);

    assert_eq!(hex::encode(key), "0100073e4d");
}

#[test]
fn mpt_current_local_root_index_key_matches_state_service_keyspace() {
    assert_eq!(hex::encode(mpt_current_local_root_index_key()), "02");
}

#[test]
fn mdbx_mpt_probe_reads_the_canonical_state_service_namespace() {
    let temp = tempfile::tempdir().expect("temporary MDBX directory");
    {
        let canonical = open_store(temp.path(), false).expect("open canonical MDBX store");
        let state_service = canonical
            .open_coordinated_namespace(MDBX_STATE_SERVICE_NAMESPACE)
            .expect("open StateService namespace");
        let mut snapshot = state_service.snapshot();
        let writer = Arc::get_mut(&mut snapshot).expect("exclusive StateService snapshot");
        writer
            .put_sync(
                mpt_current_local_root_index_key(),
                42u32.to_le_bytes().to_vec(),
            )
            .expect("write StateService height");
        writer.try_commit().expect("commit StateService height");
    }

    let output = probe_mpt_state(
        temp.path(),
        true,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        1,
        DecodeMode::Hex,
    )
    .expect("probe coordinated StateService namespace");

    assert_eq!(output["namespace"], MDBX_STATE_SERVICE_NAMESPACE);
    assert_eq!(output["height"]["decoded"]["current_local_root_index"], 42);
}

#[test]
fn ledger_transaction_key_reverses_display_hash_for_storage() {
    let key = ledger_transaction_key_from_hash(
        "0xc68d5cad0e02197dd66623373751b84b2cadf742e79aaf836b53c6999a8d264d",
    )
    .expect("transaction key");

    assert_eq!(
        hex::encode(key),
        "0b4d268d9a99c6536b83af9ae742f7ad2c4bb85137372366d67d19020ead5c8dc6"
    );
}

#[test]
fn contract_state_key_reverses_display_hash_for_storage() {
    let key = contract_state_key_from_hash("0xf970f4ccecd765b63732b821775dc38c25d74f23")
        .expect("contract state key");

    assert_eq!(
        hex::encode(key),
        "08234fd7258cc35d7721b83237b665d7ecccf470f9"
    );
}

#[test]
fn decode_nep17_account_state_balance() {
    let bytes = base64_decode("QQEhBEGk/QI=").expect("base64");

    assert_eq!(
        decode_nep17_account_balance(&bytes).unwrap().to_string(),
        "50177089"
    );
}

#[test]
fn decode_neo_account_state_reads_reward_markers() {
    let vote_to = [0x02u8; 33];
    let value = StackValue::Struct(
        neo_vm_rs::next_stack_item_id(),
        vec![
            StackValue::Integer(100),
            StackValue::Integer(151_116),
            StackValue::ByteString(vote_to.to_vec()),
            StackValue::BigInteger(BigInt::from(123456789u64).to_signed_bytes_le()),
        ],
    );
    let bytes = BinarySerializer::serialize_stack_value_default(&value).expect("serialize");

    let state = decode_neo_account_state(&bytes).expect("decode NEO account state");

    assert_eq!(state.balance, BigInt::from(100));
    assert_eq!(state.balance_height, 151_116);
    assert_eq!(
        state.vote_to_hex.as_deref(),
        Some(hex::encode(vote_to).as_str())
    );
    assert_eq!(state.last_gas_per_vote, BigInt::from(123456789u64));
}

#[test]
fn decode_raw_bigint_uses_storage_integer_format() {
    let bytes = base64_decode("n0YM").expect("base64");

    assert_eq!(decode_raw_bigint(&bytes).to_string(), "804511");
    assert_eq!(decode_raw_bigint(&[]).to_string(), "0");
}

#[test]
fn decode_hash_index_state_reads_ledger_current_block_pointer() {
    let bytes =
        base64_decode("QQIoIOVOraOmSo8jxSMutX/NUblHILNLvZdTGxS9ZpTsCjt6IQMaKAo=").expect("base64");
    let state = decode_hash_index_state(&bytes).expect("hash index state");

    assert_eq!(state.index, 665626);
    assert_eq!(
        state.hash_hex_le,
        "e54eada3a64a8f23c5232eb57fcd51b94720b34bbd97531b14bd6694ec0a3b7a"
    );
}

#[test]
fn decode_mpt_current_local_root_index_reads_little_endian_height() {
    let bytes = 474_701u32.to_le_bytes();

    assert_eq!(
        decode_mpt_current_local_root_index(&bytes).unwrap(),
        474_701
    );
}

#[test]
fn decode_mpt_state_root_record_reads_unsigned_prefix_and_ignores_witness_tail() {
    let mut bytes = vec![0x00];
    bytes.extend_from_slice(&474_701u32.to_le_bytes());
    bytes.extend_from_slice(&[0xabu8; 32]);
    bytes.push(0x00);

    let decoded = decode_mpt_state_root_record(&bytes).expect("state-root record");

    assert_eq!(decoded["version"].as_u64(), Some(0));
    assert_eq!(decoded["index"].as_u64(), Some(474_701));
    let expected_hash = "ab".repeat(32);
    assert_eq!(
        decoded["roothash_hex_le"].as_str(),
        Some(expected_hash.as_str())
    );
    assert_eq!(decoded["trailing_bytes"].as_u64(), Some(1));
}

#[test]
fn decode_transaction_state_reads_block_and_vm_state() {
    let mut tx = neo_payloads::Transaction::new();
    tx.set_system_fee(42);
    tx.set_network_fee(7);
    tx.set_valid_until_block(99);
    tx.set_signers(vec![neo_payloads::Signer::new(
        UInt160::zero(),
        neo_primitives::WitnessScope::CALLED_BY_ENTRY,
    )]);
    tx.set_script(vec![neo_vm_rs::OpCode::PUSH1.byte()]);
    tx.set_witnesses(vec![neo_payloads::Witness::new()]);
    let bytes = neo_native_contracts::LedgerContract::new()
        .serialize_persisted_transaction_state(12, VMState::FAULT, &tx)
        .expect("transaction state bytes");

    let state = decode_transaction_state(&bytes).expect("transaction state");

    assert_eq!(state.block_index, 12);
    assert_eq!(state.state, VMState::FAULT);
    let decoded_tx = state.transaction.expect("transaction");
    assert_eq!(decoded_tx.system_fee(), 42);
    assert_eq!(decoded_tx.network_fee(), 7);
    assert_eq!(decoded_tx.valid_until_block(), 99);
}

#[test]
fn cli_accepts_negative_native_contract_id() {
    let cli = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "data/mainnet",
        "--contract-id",
        "-4",
        "--key-hex",
        "0c",
    ])
    .expect("parse cli");

    assert_eq!(cli.contract_id, Some(-4));
}

#[test]
fn cli_uses_mdbx_without_a_backend_switch() {
    let cli = Cli::try_parse_from(["neo-db-probe", "--db", "data/mainnet"]).expect("parse cli");

    assert_eq!(
        cli.db.as_deref(),
        Some(std::path::Path::new("data/mainnet"))
    );
}

#[test]
fn cli_rejects_removed_storage_provider_option() {
    let error = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "data/mainnet",
        "--storage-provider",
        "mdbx",
    ])
    .expect_err("MDBX is built in and no provider switch should remain");

    assert!(error.to_string().contains("--storage-provider"));
}

#[test]
fn cli_accepts_archive_only_scrub_without_hot_database() {
    let cli = Cli::try_parse_from([
        "neo-db-probe",
        "--static-files-dir",
        "data/static",
        "--scrub-static-files",
    ])
    .expect("parse archive scrub");

    assert!(cli.db.is_none());
    assert_eq!(
        cli.static_files_dir.as_deref(),
        Some(std::path::Path::new("data/static"))
    );
    assert!(cli.scrub_static_files);
}

#[test]
fn cli_requires_hot_database_unless_running_archive_scrub() {
    let error = Cli::try_parse_from(["neo-db-probe", "--contract-id", "-4", "--key-hex", "0c"])
        .expect_err("ordinary probe without --db must fail");

    assert!(error.to_string().contains("--db"));
}

#[test]
fn archive_scrub_rejects_hot_database_and_other_probe_modes() {
    let cli = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "data/mainnet",
        "--static-files-dir",
        "data/static",
        "--scrub-static-files",
    ])
    .expect("parse conflicting scrub arguments");

    assert!(ensure_static_scrub_args(&cli).is_err());
}

#[test]
fn cli_accepts_static_archive_for_historical_transaction_replay() {
    let cli = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "data/mainnet",
        "--static-files-dir",
        "data/static",
        "--replay-tx",
        "0xc68d5cad0e02197dd66623373751b84b2cadf742e79aaf836b53c6999a8d264d",
    ])
    .expect("parse archive replay");

    assert_eq!(
        cli.static_files_dir.as_deref(),
        Some(std::path::Path::new("data/static"))
    );
}

#[test]
fn offline_ledger_factory_reconstructs_archive_only_block_and_transaction() {
    use neo_io::SerializableExtensions;
    use neo_payloads::TrimmedBlock;
    use neo_static_files::{
        StaticFileArchiveFactory, StaticFileProviderFactory, StaticRecord, StaticRow,
    };

    let mut transaction = Transaction::new();
    transaction.set_valid_until_block(100);
    transaction.set_signers(vec![neo_payloads::Signer::new(
        UInt160::zero(),
        neo_primitives::WitnessScope::CALLED_BY_ENTRY,
    )]);
    transaction.set_script(vec![neo_vm_rs::OpCode::PUSH1.byte()]);
    transaction.set_witnesses(vec![neo_payloads::Witness::new()]);
    let tx_hash = transaction.try_hash().expect("transaction hash");
    let mut header = neo_payloads::Header::new();
    header.set_index(0);
    let block = Block::from_parts(header, vec![transaction.clone()]);
    let block_hash = block.hash();

    let ledger_key = |prefix: u8, suffix: &[u8]| {
        let mut key = Vec::with_capacity(1 + suffix.len());
        key.push(prefix);
        key.extend_from_slice(suffix);
        StorageKey::new(LedgerContract::ID, key).to_array()
    };
    let rows = vec![
        StaticRow::new(ledger_key(0x09, &0u32.to_be_bytes()), block_hash.to_bytes()),
        StaticRow::new(
            ledger_key(0x05, &block_hash.to_bytes()),
            TrimmedBlock::from_block(&block)
                .expect("trimmed block")
                .to_array()
                .expect("serialize trimmed block"),
        ),
        StaticRow::new(
            ledger_key(0x0b, &tx_hash.to_bytes()),
            LedgerContract::new()
                .serialize_persisted_transaction_state(0, VMState::HALT, &transaction)
                .expect("serialize transaction state"),
        ),
    ];

    let directory = tempfile::tempdir().expect("tempdir");
    let files = StaticFileArchiveFactory::default()
        .open(&directory.path().join("ledger.static"))
        .expect("open archive");
    files
        .append(StaticRecord::new(0, rows))
        .expect("append archive record");
    drop(files);

    let hot = DataCache::new(false);
    hot.add(
        StorageKey::new(LedgerContract::ID, vec![0x09, 0, 0, 0, 0]),
        neo_storage::StorageItem::from_bytes(block_hash.to_bytes()),
    );
    hot.add(
        StorageKey::new(LedgerContract::ID, vec![0x0c]),
        neo_storage::StorageItem::from_bytes(
            LedgerContract::new()
                .serialize_hash_index_state(&block_hash, 0)
                .expect("serialize current block"),
        ),
    );
    let factory =
        open_test_offline_ledger_factory(Some(directory.path()), &hot).expect("ledger factory");
    let ledger = factory.provider(&hot);

    assert_eq!(
        ledger
            .transaction_state_by_hash(&tx_hash)
            .expect("transaction state")
            .expect("archived transaction")
            .state,
        VMState::HALT
    );
    assert_eq!(
        ledger
            .block_by_index(0)
            .expect("block")
            .expect("archived block")
            .hash(),
        block_hash
    );
}

#[test]
fn raw_ledger_probe_falls_back_to_static_archive_after_hot_miss() {
    use neo_static_files::{
        StaticFileArchiveFactory, StaticFileProviderFactory, StaticRecord, StaticRow,
    };

    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("hot");
    drop(open_store(&db, false).expect("create hot store"));
    let static_dir = directory.path().join("static");
    std::fs::create_dir(&static_dir).expect("static directory");
    let block_hash = UInt256::from_bytes(&[0x44; 32]).expect("block hash");
    write_storage_value(
        &db,
        LedgerContract::ID,
        vec![0x09, 0, 0, 0, 0],
        block_hash.to_bytes(),
    )
    .expect("write hot block hash");
    write_storage_value(
        &db,
        LedgerContract::ID,
        vec![0x0c],
        LedgerContract::new()
            .serialize_hash_index_state(&block_hash, 0)
            .expect("serialize current block"),
    )
    .expect("write hot current block");
    let suffix = vec![0x0b, 0x11, 0x22, 0x33];
    let key = StorageKey::new(LedgerContract::ID, suffix.clone()).to_array();
    let files = StaticFileArchiveFactory::default()
        .open(&static_dir.join("ledger.static"))
        .expect("open archive");
    files
        .append(StaticRecord::new(
            0,
            vec![
                StaticRow::new(
                    StorageKey::new(LedgerContract::ID, vec![0x09, 0, 0, 0, 0]).to_array(),
                    block_hash.to_bytes(),
                ),
                StaticRow::new(key, b"archived".to_vec()),
            ],
        ))
        .expect("append archive row");
    drop(files);

    assert_eq!(
        read_storage_value(&db, Some(&static_dir), LedgerContract::ID, suffix,)
            .expect("read routed value"),
        Some(b"archived".to_vec())
    );
}

#[test]
fn scrub_static_archive_reports_verified_tip() {
    use neo_static_files::{
        StaticFileArchiveFactory, StaticFileProviderFactory, StaticRecord, StaticRow,
    };

    let directory = tempfile::tempdir().expect("tempdir");
    let files = StaticFileArchiveFactory::default()
        .open(&directory.path().join("ledger.static"))
        .expect("open archive");
    files
        .append(StaticRecord::new(
            0,
            vec![StaticRow::new(b"key".to_vec(), b"value".to_vec())],
        ))
        .expect("append archive row");
    drop(files);

    let output = scrub_static_archive(directory.path()).expect("scrub archive");

    assert_eq!(output["status"], "ok");
    assert_eq!(output["tip"], 0);
}

#[test]
fn scrub_static_archive_rejects_a_missing_archive_without_creating_it() {
    let directory = tempfile::tempdir().expect("tempdir");
    let archive_path = directory.path().join("ledger.static");

    let error = scrub_static_archive(directory.path()).expect_err("missing archive must fail");

    assert!(error.to_string().contains("does not exist"));
    assert!(!archive_path.exists());
}

#[test]
fn cli_accepts_neo_account_decode_mode() {
    let cli = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "data/mainnet",
        "--contract-id",
        "-5",
        "--key-hex",
        "14",
        "--decode",
        "neo-account",
    ])
    .expect("parse cli");

    assert!(matches!(cli.decode, DecodeMode::NeoAccount));
}

#[test]
fn cli_accepts_contract_storage_dump_without_key() {
    let cli = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "data/mainnet",
        "--dump-contract-storage",
        "--contract-id",
        "33",
    ])
    .expect("parse cli");

    assert!(cli.dump_contract_storage);
    assert_eq!(cli.contract_id, Some(33));
}

#[test]
fn cli_accepts_raw_transaction_replay_with_block_context() {
    let cli = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "data/mainnet",
        "--replay-raw-tx-base64",
        "AQID",
        "--replay-block-base64",
        "BAUG",
    ])
    .expect("parse cli");

    assert_eq!(cli.replay_raw_tx_base64.as_deref(), Some("AQID"));
    assert_eq!(cli.replay_block_base64.as_deref(), Some("BAUG"));
}

#[test]
fn cli_accepts_mpt_state_probe_without_contract_key() {
    let cli = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "Data_MPT_validate_334F454E",
        "--mpt-state-height",
        "--mpt-state-root",
        "474701",
    ])
    .expect("parse cli");

    assert!(cli.mpt_state_height);
    assert_eq!(cli.mpt_state_root, Some(474_701));
}

#[test]
fn cli_accepts_mpt_key_probe_with_contract_key() {
    let cli = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "Data_MPT_validate_334F454E",
        "--mpt-key-root",
        "5549",
        "--contract-id",
        "-6",
        "--key-hex",
        "14",
        "--decode",
        "nep17-account",
    ])
    .expect("parse cli");

    assert_eq!(cli.mpt_key_root, Some(5549));
    assert_eq!(cli.contract_id, Some(-6));
    assert!(matches!(cli.decode, DecodeMode::Nep17Account));
    ensure_mpt_probe_args(&cli).expect("valid mpt key probe");
}

#[test]
fn mpt_key_probe_requires_exactly_one_key_argument() {
    let missing_key = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "Data_MPT_validate_334F454E",
        "--mpt-key-root",
        "5549",
        "--contract-id",
        "-6",
    ])
    .expect("parse cli");
    let err = ensure_mpt_probe_args(&missing_key).expect_err("missing key should fail");
    assert!(err.to_string().contains("exactly one"));

    let missing_contract = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "Data_MPT_validate_334F454E",
        "--mpt-key-root",
        "5549",
        "--key-hex",
        "14",
    ])
    .expect("parse cli");
    let err = ensure_mpt_probe_args(&missing_contract).expect_err("missing contract should fail");
    assert!(err.to_string().contains("--contract-id is required"));
}

#[test]
fn cli_accepts_mpt_contract_dump_with_contract_id() {
    let cli = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "Data_MPT_validate_334F454E",
        "--mpt-dump-contract-root",
        "5549",
        "--contract-id",
        "-1",
        "--dump-limit",
        "20",
    ])
    .expect("parse cli");

    assert_eq!(cli.mpt_dump_contract_root, Some(5549));
    assert_eq!(cli.contract_id, Some(-1));
    ensure_mpt_probe_args(&cli).expect("valid mpt contract dump");
}

#[test]
fn cli_accepts_mpt_root_dump_without_contract_id() {
    let cli = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "Data_MPT_validate_334F454E",
        "--mpt-dump-root",
        "5549",
        "--dump-limit",
        "100",
    ])
    .expect("parse cli");

    assert_eq!(cli.mpt_dump_root, Some(5549));
    ensure_mpt_probe_args(&cli).expect("valid mpt root dump");
}

#[test]
fn mpt_state_probe_rejects_chain_storage_arguments() {
    let cli = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "Data_MPT_validate_334F454E",
        "--mpt-state-height",
        "--contract-id",
        "-4",
        "--key-hex",
        "0c",
    ])
    .expect("parse cli");

    let err = ensure_mpt_probe_args(&cli).expect_err("mixed probe modes should fail");
    assert!(
        err.to_string()
            .contains("cannot be combined with chain storage probe arguments"),
        "{err}"
    );
}

fn offline_empty_block_rows(
    height: u32,
    nonce: u64,
) -> (Block, UInt256, Vec<(StorageKey, Vec<u8>)>) {
    use neo_io::SerializableExtensions;
    use neo_payloads::TrimmedBlock;

    let mut header = neo_payloads::Header::new();
    header.set_index(height);
    header.set_nonce(nonce);
    let block = Block::from_parts(header, Vec::new());
    let hash = block.hash();
    let mut block_hash_suffix = vec![0x09];
    block_hash_suffix.extend_from_slice(&height.to_be_bytes());
    let mut block_suffix = vec![0x05];
    block_suffix.extend_from_slice(&hash.to_bytes());
    let rows = vec![
        (
            StorageKey::new(LedgerContract::ID, block_hash_suffix),
            hash.to_bytes(),
        ),
        (
            StorageKey::new(LedgerContract::ID, block_suffix),
            TrimmedBlock::from_block(&block)
                .expect("trimmed block")
                .to_array()
                .expect("serialize trimmed block"),
        ),
    ];
    (block, hash, rows)
}

fn add_hot_rows<B: neo_storage::CacheRead>(hot: &DataCache<B>, rows: &[(StorageKey, Vec<u8>)]) {
    for (key, value) in rows {
        hot.add(
            key.clone(),
            neo_storage::StorageItem::from_bytes(value.clone()),
        );
    }
}

fn set_hot_tip<B: neo_storage::CacheRead>(hot: &DataCache<B>, hash: &UInt256, height: u32) {
    hot.add(
        StorageKey::new(LedgerContract::ID, vec![0x0c]),
        neo_storage::StorageItem::from_bytes(
            LedgerContract::new()
                .serialize_hash_index_state(hash, height)
                .expect("serialize current block"),
        ),
    );
}

fn write_static_records(
    directory: &std::path::Path,
    records: Vec<(u32, Vec<(StorageKey, Vec<u8>)>)>,
) {
    use neo_static_files::{
        StaticFileArchiveFactory, StaticFileProviderFactory, StaticRecord, StaticRow,
    };

    let files = StaticFileArchiveFactory::default()
        .open(&directory.join("ledger.static"))
        .expect("open archive");
    files
        .append_batch(
            records
                .into_iter()
                .map(|(height, rows)| {
                    StaticRecord::new(
                        height,
                        rows.into_iter()
                            .map(|(key, value)| StaticRow::new(key.to_array(), value))
                            .collect(),
                    )
                })
                .collect(),
        )
        .expect("append archive records");
}

fn open_test_offline_ledger_factory<B: CacheRead>(
    static_files_dir: Option<&Path>,
    snapshot: &DataCache<B>,
) -> Result<HotColdLedgerProviderFactory<OptionalStaticLedgerProvider>> {
    let metadata = neo_storage::persistence::providers::memory_store::MemoryStore::new();
    open_offline_ledger_factory(static_files_dir, &metadata, snapshot)
}

#[test]
fn offline_ledger_factory_repairs_lagging_archive_from_hot_canonical_rows() {
    let directory = tempfile::tempdir().expect("tempdir");
    let (block0, _, rows0) = offline_empty_block_rows(0, 10);
    let (block1, hash1, rows1) = offline_empty_block_rows(1, 11);
    let hot = DataCache::new(false);
    add_hot_rows(&hot, &rows0);
    add_hot_rows(&hot, &rows1);
    set_hot_tip(&hot, &hash1, 1);
    write_static_records(directory.path(), vec![(0, rows0)]);

    let factory = open_test_offline_ledger_factory(Some(directory.path()), &hot)
        .expect("reconcile lagging archive");
    let provider = factory.provider(&hot);

    assert_eq!(
        provider
            .block_by_index(0)
            .expect("block 0")
            .expect("stored block 0")
            .hash(),
        block0.hash()
    );
    assert_eq!(
        provider
            .block_by_index(1)
            .expect("block 1")
            .expect("stored block 1")
            .hash(),
        block1.hash()
    );
    drop(provider);
    drop(factory);
    assert_eq!(
        open_existing_static_ledger_archive(directory.path())
            .expect("reopen archive")
            .tip(),
        Some(1)
    );
}

#[test]
fn offline_ledger_factory_truncates_archive_ahead_of_hot_tip() {
    let directory = tempfile::tempdir().expect("tempdir");
    let (_, hash0, rows0) = offline_empty_block_rows(0, 20);
    let (_, _, rows1) = offline_empty_block_rows(1, 21);
    let hot = DataCache::new(false);
    add_hot_rows(&hot, &rows0);
    set_hot_tip(&hot, &hash0, 0);
    write_static_records(directory.path(), vec![(0, rows0), (1, rows1)]);

    drop(
        open_test_offline_ledger_factory(Some(directory.path()), &hot)
            .expect("reconcile ahead archive"),
    );

    assert_eq!(
        open_existing_static_ledger_archive(directory.path())
            .expect("reopen archive")
            .tip(),
        Some(0)
    );
}

#[test]
fn offline_ledger_factory_rejects_archive_fork_mismatch() {
    let directory = tempfile::tempdir().expect("tempdir");
    let (_, hot_hash, hot_rows) = offline_empty_block_rows(0, 30);
    let (_, _, archive_rows) = offline_empty_block_rows(0, 31);
    let hot = DataCache::new(false);
    add_hot_rows(&hot, &hot_rows);
    set_hot_tip(&hot, &hot_hash, 0);
    write_static_records(directory.path(), vec![(0, archive_rows)]);

    let error = open_test_offline_ledger_factory(Some(directory.path()), &hot)
        .expect_err("forked archive must fail");

    assert!(error.to_string().contains("fork mismatch"), "{error}");
}

#[test]
fn offline_ledger_factory_does_not_truncate_archive_for_uninitialized_hot_store() {
    let directory = tempfile::tempdir().expect("tempdir");
    let (_, _, archive_rows) = offline_empty_block_rows(0, 40);
    write_static_records(directory.path(), vec![(0, archive_rows)]);
    let hot = DataCache::new(false);

    let error = open_test_offline_ledger_factory(Some(directory.path()), &hot)
        .expect_err("uninitialized hot store must fail");

    assert!(error.to_string().contains("uninitialized"), "{error}");
    assert_eq!(
        open_existing_static_ledger_archive(directory.path())
            .expect("reopen archive")
            .tip(),
        Some(0)
    );
}

#[test]
fn offline_ledger_factory_rejects_corrupt_hot_tip_without_touching_archive() {
    let directory = tempfile::tempdir().expect("tempdir");
    let (_, hash, rows) = offline_empty_block_rows(0, 41);
    write_static_records(directory.path(), vec![(0, rows.clone())]);
    let hot = DataCache::new(false);
    add_hot_rows(&hot, &rows);
    hot.add(
        StorageKey::new(LedgerContract::ID, vec![0x0c]),
        neo_storage::StorageItem::from_bytes(vec![0xff]),
    );
    let archive =
        open_existing_static_ledger_archive(directory.path()).expect("hold archive writer lease");

    let error = open_test_offline_ledger_factory(Some(directory.path()), &hot)
        .expect_err("corrupt hot tip must fail");

    assert!(error.to_string().contains("read hot Ledger tip"), "{error}");
    assert_eq!(
        archive
            .provider()
            .block_hash_by_index(0)
            .expect("archived hash"),
        Some(hash)
    );
}
