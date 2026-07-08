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
    let value = StackValue::Struct(vec![
        StackValue::Integer(100),
        StackValue::Integer(151_116),
        StackValue::ByteString(vote_to.to_vec()),
        StackValue::BigInteger(BigInt::from(123456789u64).to_signed_bytes_le()),
    ]);
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
fn cli_defaults_to_mdbx_storage_provider_in_production_build() {
    let cli = Cli::try_parse_from(["neo-db-probe", "--db", "data/mainnet"]).expect("parse cli");

    assert_eq!(cli.storage_provider, StorageProviderArg::Mdbx);
}

#[test]
fn cli_accepts_explicit_rocksdb_storage_provider() {
    let cli = Cli::try_parse_from([
        "neo-db-probe",
        "--db",
        "data/mainnet",
        "--storage-provider",
        "rocksdb",
    ])
    .expect("parse cli");

    assert_eq!(cli.storage_provider, StorageProviderArg::Rocksdb);
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
