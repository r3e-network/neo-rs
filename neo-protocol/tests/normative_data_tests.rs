//! Normative scenarios retained from core header, block, wire-parity, and
//! transaction serialization tests, plus low-layer migration regressions.
use neo_crypto::{Crypto, MerkleTree};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::{OracleResponseCode, SerializablePayload, UInt160, UInt256, WitnessScope};
use neo_protocol::execution::{BlockHeader, conversions};
use neo_protocol::payloads::{
    Block, Conflicts, Header, NotaryAssisted, OracleResponse, Signer, Transaction,
    TransactionAttribute,
};
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_protocol::witness::{Witness, WitnessData};

fn encode<T: Serializable>(value: &T) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    Serializable::serialize(value, &mut writer).unwrap();
    writer.into_bytes()
}
fn decode<T: Serializable>(bytes: &[u8]) -> neo_io::IoResult<T> {
    T::deserialize(&mut MemoryReader::new(bytes))
}
fn sample_witness() -> Witness {
    Witness::new_with_scripts(vec![0x0c, 0x40], vec![0x41, 0x56, 0xe7, 0xb3, 0x27])
}
fn sample_header() -> Header {
    let mut header = Header::new();
    header.set_prev_hash(UInt256::from([0x11; 32]));
    header.set_merkle_root(UInt256::from([0x22; 32]));
    header.set_timestamp(1_700_000_000_000);
    header.set_nonce(0x0102_0304_0506_0708);
    header.set_index(1_074_782);
    header.set_primary_index(3);
    header.set_next_consensus(UInt160::from([0x33; 20]));
    header.witness = sample_witness();
    header
}
fn sample_tx() -> Transaction {
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(100_000_000);
    tx.set_network_fee(1);
    tx.set_valid_until_block(0x0102_0304);
    tx.add_signer(Signer::new(UInt160::zero(), WitnessScope::NONE));
    tx.set_attributes(vec![]);
    tx.set_script(vec![0x11]);
    tx.add_witness(Witness::empty());
    tx
}
fn oversized_tx() -> Transaction {
    let mut tx = sample_tx();
    tx.set_script(vec![0x21; u16::MAX as usize + 1]);
    tx
}
fn sample_block() -> Block {
    Block {
        header: sample_header(),
        transactions: vec![],
    }
}

// Existing core/tests/header_wire_parity.rs scenarios.
#[test]
fn p2p_header_and_block_header_encode_the_same_bytes() {
    let payload = sample_header();
    let execution = conversions::convert_payload_header(&payload);
    let bytes = encode(&payload);
    assert_eq!(bytes, encode(&execution));
    assert_eq!(payload.size(), execution.size());
    assert_eq!(
        BlockHeader::UNSIGNED_WIRE_SIZE,
        4 + 32 + 32 + 8 + 8 + 4 + 1 + 20
    );
    let from_payload: BlockHeader = decode(&bytes).unwrap();
    let from_execution: Header = decode(&encode(&execution)).unwrap();
    assert_eq!(encode(&from_payload), bytes);
    assert_eq!(encode(&from_execution), bytes);
    assert_eq!(from_payload.hash(), execution.hash());
}
#[test]
fn either_type_rejects_a_second_witness() {
    let mut bytes = encode(&conversions::convert_payload_header(&sample_header()));
    let count_at = BlockHeader::UNSIGNED_WIRE_SIZE;
    assert_eq!(bytes[count_at], 1);
    bytes[count_at] = 2;
    assert!(decode::<BlockHeader>(&bytes).is_err());
    assert!(decode::<Header>(&bytes).is_err());
}

// Existing payload Header's three normative tests (state tests belong to ledger).
#[test]
fn try_hash_matches_legacy_hash_for_valid_header() {
    let mut header = sample_header();
    let mut legacy = header.clone();
    assert_eq!(header.try_hash().unwrap(), Header::hash(&mut legacy));
}
#[test]
fn try_get_hash_data_matches_serialize_unsigned() {
    let header = sample_header();
    let execution = conversions::convert_payload_header(&header);
    let mut writer = BinaryWriter::new();
    execution.serialize_unsigned(&mut writer).unwrap();
    assert_eq!(header.try_get_hash_data().unwrap(), writer.into_bytes());
}
#[test]
fn iverifiable_header_hash_uses_try_hash() {
    let header = sample_header();
    let expected = header.clone().try_hash().unwrap();
    assert_eq!(
        <Header as neo_primitives::Verifiable>::hash(&header).unwrap(),
        expected
    );
}

// Existing payload Block's six normative tests (cache validation is ledger-owned).
#[test]
fn block_try_hash_delegates_to_header() {
    let mut block = sample_block();
    let mut header = block.header.clone();
    assert_eq!(block.try_hash().unwrap(), header.try_hash().unwrap());
}
#[test]
fn iverifiable_block_hash_uses_try_hash() {
    let block = sample_block();
    let expected = block.clone().try_hash().unwrap();
    assert_eq!(
        <Block as neo_primitives::Verifiable>::hash(&block).unwrap(),
        expected
    );
}
#[test]
fn block_try_get_hash_data_matches_header_hash_data() {
    let block = sample_block();
    assert_eq!(
        block.try_get_hash_data().unwrap(),
        block.header.try_get_hash_data().unwrap()
    );
}
#[test]
fn try_rebuild_merkle_root_rejects_unserializable_transaction_hash() {
    let mut block = sample_block();
    block.transactions.push(oversized_tx());
    assert!(block.try_rebuild_merkle_root().is_err());
}
#[test]
fn verify_merkle_root_rejects_unserializable_transaction_hash() {
    let mut block = sample_block();
    block.transactions.push(oversized_tx());
    block.header.set_merkle_root(UInt256::default());
    assert!(!block.verify_merkle_root());
}
#[test]
fn duplicate_transaction_check_rejects_unserializable_transaction_hash() {
    let mut block = sample_block();
    block.transactions.push(oversized_tx());
    assert!(!block.verify_no_duplicate_transactions());
}

// Retained core/tests/transaction_serialization_compatibility_tests.rs scenarios.
mod serialization_compatibility_tests {
    use super::*;
    fn csharp_bytes() -> Vec<u8> {
        hex::decode(concat!(
            "00",
            "04030201",
            "00e1f50500000000",
            "0100000000000000",
            "04030201",
            "01000000000000000000000000000000000000000000",
            "00",
            "0111",
            "010000"
        ))
        .unwrap()
    }
    #[test]
    fn test_transaction_serialize_simple_matches_csharp() {
        let tx = sample_tx();
        let bytes = tx.to_bytes();
        assert_eq!(bytes, csharp_bytes());
        let decoded = Transaction::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.version(), 0);
        assert_eq!(decoded.nonce(), 0x0102_0304);
        assert_eq!(decoded.system_fee(), 100_000_000);
        assert_eq!(decoded.network_fee(), 1);
        assert_eq!(decoded.valid_until_block(), 0x0102_0304);
        assert_eq!(decoded.script(), vec![0x11]);
    }
    #[test]
    fn test_transaction_hash_calculation() {
        let tx = sample_tx();
        let hash = tx.hash();
        assert_eq!(hash, tx.hash());
        let mut other = sample_tx();
        other.set_nonce(0x0102_0305);
        assert_ne!(hash, other.hash());
        assert_eq!(
            hash,
            UInt256::from(Crypto::sha256(&tx.try_get_hash_data().unwrap()))
        );
    }
    #[test]
    fn test_transaction_size_calculation() {
        let tx = sample_tx();
        assert_eq!(tx.size(), tx.to_bytes().len());
    }
    #[test]
    fn test_fee_per_byte_calculation() {
        let mut tx = sample_tx();
        tx.set_network_fee(1000);
        assert!(tx.network_fee() as f64 / tx.size() as f64 > 0.0);
        assert_eq!(tx.fee_per_byte(), tx.network_fee() / tx.size() as i64);
    }
    #[test]
    fn test_deserialize_csharp_transaction() {
        let tx = Transaction::from_bytes(&csharp_bytes()).unwrap();
        assert_eq!(tx.version(), 0);
        assert_eq!(tx.nonce(), 0x0102_0304);
        assert_eq!(tx.system_fee(), 100_000_000);
        assert_eq!(tx.network_fee(), 1);
        assert_eq!(tx.valid_until_block(), 0x0102_0304);
        assert_eq!(tx.signers().len(), 1);
        assert_eq!(tx.script(), vec![0x11]);
    }
    #[test]
    fn test_distinct_signers_validation() {
        let mut tx = sample_tx();
        let account =
            UInt160::from_bytes(&[9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0])
                .unwrap();
        tx.set_signers(vec![
            Signer::new(account, WitnessScope::GLOBAL),
            Signer::new(account, WitnessScope::CALLED_BY_ENTRY),
        ]);
        tx.set_witnesses(vec![Witness::empty(), Witness::empty()]);
        let bytes = tx.to_bytes();
        assert!(!bytes.is_empty());
        assert!(Transaction::from_bytes(&bytes).is_err());
    }
    #[test]
    fn test_multiple_signers_serialization() {
        let mut tx = sample_tx();
        tx.set_network_fee(1000);
        tx.set_signers(vec![]);
        tx.set_witnesses(vec![]);
        for i in 0..3 {
            let mut bytes = [0u8; 20];
            bytes[0] = i + 1;
            tx.add_signer(Signer::new(
                UInt160::from_bytes(&bytes).unwrap(),
                WitnessScope::CALLED_BY_ENTRY,
            ));
            tx.add_witness(Witness::empty());
        }
        tx.set_script(vec![0x11, 0x12, 0x13]);
        let decoded = Transaction::from_bytes(&tx.to_bytes()).unwrap();
        assert_eq!(decoded.signers().len(), 3);
        assert_eq!(decoded.witnesses().len(), 3);
        assert_eq!(decoded.script(), vec![0x11, 0x12, 0x13]);
    }
    #[test]
    fn test_empty_transaction_handling() {
        let tx = Transaction::new();
        assert_eq!(tx.version(), 0);
        assert_eq!(tx.system_fee(), 0);
        assert_eq!(tx.network_fee(), 0);
        assert_eq!(tx.valid_until_block(), 0);
        assert!(tx.signers().is_empty());
        assert!(tx.attributes().is_empty());
        assert!(tx.script().is_empty());
        assert!(tx.witnesses().is_empty());
        assert!(!tx.to_bytes().is_empty());
    }
    #[test]
    fn test_max_values_serialization() {
        let mut tx = sample_tx();
        tx.set_version(u8::MAX);
        tx.set_nonce(u32::MAX);
        tx.set_system_fee(i64::MAX);
        tx.set_network_fee(i64::MAX);
        tx.set_valid_until_block(u32::MAX);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
        tx.set_script(vec![0xFF]);
        assert!(!tx.to_bytes().is_empty());
        // Serialization remains permissive; decoding retains version/fee rejection.
        assert!(Transaction::from_bytes(&tx.to_bytes()).is_err());
    }
}

#[test]
fn header_hash_and_field_order_are_the_common_single_sha256_preimage() {
    let mut header = sample_header();
    let bytes = header.try_get_hash_data().unwrap();
    assert_eq!(&bytes[0..4], &0u32.to_le_bytes());
    assert_eq!(&bytes[4..36], &[0x11; 32]);
    assert_eq!(&bytes[36..68], &[0x22; 32]);
    assert_eq!(&bytes[68..76], &1_700_000_000_000u64.to_le_bytes());
    assert_eq!(&bytes[76..84], &0x0102_0304_0506_0708u64.to_le_bytes());
    assert_eq!(&bytes[84..88], &1_074_782u32.to_le_bytes());
    assert_eq!(bytes[88], 3);
    assert_eq!(&bytes[89..109], &[0x33; 20]);
    assert_eq!(
        header.try_hash().unwrap(),
        UInt256::from(Crypto::sha256(&bytes))
    );
}
#[test]
fn transaction_hash_and_size_caches_keep_mutation_contract() {
    let mut tx = sample_tx();
    let hash = tx.hash();
    let size = tx.size();
    tx.set_witnesses(vec![Witness::new_with_scripts(vec![1; 100], vec![])]);
    assert_eq!(tx.hash(), hash);
    assert!(tx.size() > size);
    tx.set_nonce(tx.nonce() + 1);
    assert_ne!(tx.hash(), hash);
    let cached_size = tx.size();
    WitnessData::witnesses_mut(&mut tx)[0].invocation_script = vec![2; 200];
    assert_eq!(tx.size(), cached_size); // Do not strengthen historical trait mutation.
    let clone = tx.clone();
    assert_eq!(clone.size(), clone.to_bytes().len());
    assert_ne!(clone.size(), cached_size);
}
#[test]
fn signer_hash_order_and_rule_order_remain_normative() {
    use neo_io::witness_rule::{WitnessCondition, WitnessRule, WitnessRuleAction};
    use std::hash::{Hash, Hasher};
    let account = UInt160::from([9; 20]);
    let mut signer = Signer::new(
        account,
        WitnessScope::CALLED_BY_ENTRY
            | WitnessScope::CUSTOM_CONTRACTS
            | WitnessScope::WITNESS_RULES,
    );
    signer.allowed_contracts = vec![UInt160::from([1; 20]), UInt160::from([2; 20])];
    let tail = WitnessRule::new(
        WitnessRuleAction::Deny,
        WitnessCondition::Boolean { value: false },
    );
    signer.rules = vec![tail.clone()];
    let rules = signer.get_all_rules();
    assert_eq!(rules[0].condition, WitnessCondition::CalledByEntry);
    assert_eq!(
        rules[1].condition,
        WitnessCondition::ScriptHash {
            hash: signer.allowed_contracts[0]
        }
    );
    assert_eq!(
        rules[2].condition,
        WitnessCondition::ScriptHash {
            hash: signer.allowed_contracts[1]
        }
    );
    assert_eq!(rules[3], tail);
    let mut actual = std::collections::hash_map::DefaultHasher::new();
    signer.hash(&mut actual);
    let mut expected = std::collections::hash_map::DefaultHasher::new();
    signer.account.hash(&mut expected);
    signer.scopes.hash(&mut expected);
    assert_eq!(actual.finish(), expected.finish());
    let json = signer.to_json();
    assert_eq!(Signer::from_json(&json).unwrap(), signer);
}
#[test]
fn block_witness_access_is_header_only_but_payload_count_is_aggregate() {
    let mut block = sample_block();
    block.transactions = vec![sample_tx(), sample_tx()];
    assert_eq!(WitnessData::witnesses(&block).len(), 1);
    assert_eq!(SerializablePayload::witness_count(&block), 3);
    assert_eq!(
        SerializablePayload::invocation_script(&block, 0),
        block.header.witness.invocation_script
    );
    assert!(SerializablePayload::invocation_script(&block, 3).is_empty());
    assert!(SerializablePayload::verification_script(&block, 3).is_empty());
    WitnessData::witnesses_mut(&mut block)[0].invocation_script = vec![5];
    assert_eq!(block.header.witness.invocation_script, vec![5]);
}
#[test]
fn block_codec_checks_duplicates_and_merkle_root_without_state() {
    let tx = sample_tx();
    let mut header = sample_header();
    header.set_merkle_root(tx.hash());
    let block = Block {
        header: header.clone(),
        transactions: vec![tx.clone()],
    };
    assert!(decode::<Block>(&encode(&block)).is_ok());
    let duplicated = Block {
        header,
        transactions: vec![tx.clone(), tx],
    };
    assert!(!duplicated.verify_no_duplicate_transactions());
    let error = decode::<Block>(&encode(&duplicated)).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Duplicate transactions on a block:")
    );
    let mut wrong = block;
    wrong.header.set_merkle_root(UInt256::zero());
    assert!(
        decode::<Block>(&encode(&wrong))
            .unwrap_err()
            .to_string()
            .contains("The computed Merkle root does not match the expected value.")
    );
    assert_eq!(
        MerkleTree::compute_root(&[]).unwrap_or_default(),
        UInt256::zero()
    );
}
#[test]
fn block_wire_allows_more_than_two_mebibytes() {
    let mut block = Block::new();
    for nonce in 0..33 {
        let mut tx = sample_tx();
        tx.set_nonce(nonce);
        tx.set_script(vec![0x21; u16::MAX as usize]);
        block.transactions.push(tx);
    }
    block.try_rebuild_merkle_root().unwrap();
    let bytes = encode(&block);
    assert!(bytes.len() > 2_097_152);
    assert_eq!(decode::<Block>(&bytes).unwrap().transactions.len(), 33);
}
#[test]
fn oracle_wire_limits_and_deserialize_only_non_success_rule() {
    let valid = OracleResponse::new(7, OracleResponseCode::Success, vec![1; u16::MAX as usize]);
    let valid_bytes = encode(&valid);
    assert_eq!(&valid_bytes[..8], &7u64.to_le_bytes());
    assert_eq!(valid_bytes[8], OracleResponseCode::Success.to_byte());
    assert_eq!(&valid_bytes[9..12], &[0xfd, 0xff, 0xff]);
    assert_eq!(decode::<OracleResponse>(&valid_bytes).unwrap(), valid);
    let empty_error = OracleResponse::new(7, OracleResponseCode::Error, vec![]);
    assert_eq!(
        decode::<OracleResponse>(&encode(&empty_error)).unwrap(),
        empty_error
    );
    let mut oversized_writer = BinaryWriter::new();
    oversized_writer.write_u64(7).unwrap();
    oversized_writer
        .write_u8(OracleResponseCode::Success.to_byte())
        .unwrap();
    oversized_writer
        .write_var_bytes(&vec![1; u16::MAX as usize + 1])
        .unwrap();
    assert!(decode::<OracleResponse>(&oversized_writer.into_bytes()).is_err());
    let oversized = OracleResponse::new(
        7,
        OracleResponseCode::Success,
        vec![1; u16::MAX as usize + 1],
    );
    let error = Serializable::serialize(&oversized, &mut BinaryWriter::new()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Result exceeds maximum length of 65535 (got 65536)")
    );
    let non_success = OracleResponse::new(7, OracleResponseCode::Error, vec![1]);
    let encoded = encode(&non_success); // Encoding remains allowed.
    assert!(
        decode::<OracleResponse>(&encoded)
            .unwrap_err()
            .to_string()
            .contains("Non-success response cannot have result")
    );
    let mut invalid = encode(&OracleResponse::new(7, OracleResponseCode::Success, vec![]));
    invalid[8] = 0x7f;
    assert!(
        decode::<OracleResponse>(&invalid)
            .unwrap_err()
            .to_string()
            .contains("Invalid response code")
    );
}
#[test]
fn attribute_body_widths_and_transaction_multiplicity_are_preserved() {
    assert_eq!(encode(&TransactionAttribute::HighPriority), vec![0x01]);
    assert_eq!(
        encode(&TransactionAttribute::not_valid_before(0x0102_0304)),
        vec![0x20, 4, 3, 2, 1]
    );
    assert_eq!(
        encode(&TransactionAttribute::NotaryAssisted(NotaryAssisted::new(
            3
        ))),
        vec![0x22, 3]
    );
    assert_eq!(
        encode(&TransactionAttribute::Conflicts(Conflicts::new(
            UInt256::from([5; 32])
        ))),
        [vec![0x21], vec![5; 32]].concat()
    );
    let mut tx = sample_tx();
    tx.set_attributes(vec![
        TransactionAttribute::HighPriority,
        TransactionAttribute::HighPriority,
    ]);
    assert!(
        Transaction::from_bytes(&tx.to_bytes())
            .unwrap_err()
            .to_string()
            .contains("Duplicate attribute")
    );
    tx.set_attributes(vec![
        TransactionAttribute::Conflicts(Conflicts::new(UInt256::zero())),
        TransactionAttribute::Conflicts(Conflicts::new(UInt256::zero())),
    ]);
    assert!(Transaction::from_bytes(&tx.to_bytes()).is_ok());
    tx.set_attributes(vec![
        TransactionAttribute::Conflicts(Conflicts::new(
            UInt256::zero()
        ));
        16
    ]);
    assert!(Transaction::from_bytes(&tx.to_bytes()).is_err()); // One signer leaves room for 15 attributes.
}
#[test]
fn transaction_json_retains_fee_strings_base64_and_explicit_address_version() {
    let tx = sample_tx();
    let mut settings = ProtocolSettings::default_settings();
    let default_sender = tx.to_json(&settings)["sender"].clone();
    settings.address_version = settings.address_version.wrapping_add(1);
    let json = tx.to_json(&settings);
    assert_ne!(json["sender"], default_sender);
    assert!(Transaction::new().to_json(&settings)["sender"].is_null());
    assert_eq!(json["sysfee"], "100000000");
    assert_eq!(json["netfee"], "1");
    assert_eq!(json["script"], "EQ==");
    assert_eq!(json["witnesses"][0]["invocation"], "");
    assert_eq!(
        json["sender"],
        neo_primitives::base58_check::encode_address_payload(
            settings.address_version,
            &UInt160::zero().to_array()
        )
    );
}
#[test]
fn witness_hash160_json_and_deserialize_limits_keep_the_original_contract() {
    let mut witness = Witness::new_with_scripts(vec![1; 1024], vec![2; 1024]);
    assert_eq!(
        witness.script_hash(),
        UInt160::from(Crypto::hash160(&witness.verification_script))
    );
    let decoded: Witness = decode(&encode(&witness)).unwrap();
    assert_eq!(decoded.invocation_script, witness.invocation_script);
    assert_eq!(decoded.verification_script, witness.verification_script);
    assert_eq!(
        Witness::new_with_scripts(vec![0x11], vec![0x12]).to_json(),
        serde_json::json!({"invocation": "EQ==", "verification": "Eg=="})
    );
    witness.invocation_script.push(1);
    assert!(decode::<Witness>(&encode(&witness)).is_err());
    witness.invocation_script.clear();
    witness.verification_script.push(2);
    assert!(decode::<Witness>(&encode(&witness)).is_err());
}
#[test]
fn payload_header_setters_invalidate_hash_but_witness_mutation_does_not() {
    let mut header = sample_header();
    let hash = header.try_hash().unwrap();
    let preimage = header.try_get_hash_data().unwrap();
    header.witness = Witness::new_with_scripts(vec![9], vec![8]);
    assert_eq!(header.try_hash().unwrap(), hash);
    assert_eq!(header.try_get_hash_data().unwrap(), preimage);
    header.set_nonce(header.nonce() + 1);
    let fresh = UInt256::from(Crypto::sha256(&header.try_get_hash_data().unwrap()));
    assert_ne!(fresh, hash);
    assert_eq!(header.try_hash().unwrap(), fresh);
}
#[test]
fn transaction_decode_retains_version_fee_script_and_witness_rejections() {
    let mut tx = sample_tx();
    tx.set_version(1);
    assert!(
        decode::<Transaction>(&encode(&tx))
            .unwrap_err()
            .to_string()
            .contains("Invalid transaction version")
    );
    tx.set_version(0);
    tx.set_system_fee(-1);
    assert!(
        decode::<Transaction>(&encode(&tx))
            .unwrap_err()
            .to_string()
            .contains("Invalid system fee")
    );
    tx.set_system_fee(0);
    tx.set_network_fee(-1);
    assert!(
        decode::<Transaction>(&encode(&tx))
            .unwrap_err()
            .to_string()
            .contains("Invalid network fee")
    );
    tx.set_system_fee(i64::MAX);
    tx.set_network_fee(1);
    assert!(
        decode::<Transaction>(&encode(&tx))
            .unwrap_err()
            .to_string()
            .contains("Combined fee overflow")
    );
    tx.set_system_fee(0);
    tx.set_script(vec![]);
    assert!(
        decode::<Transaction>(&encode(&tx))
            .unwrap_err()
            .to_string()
            .contains("Script length cannot be zero")
    );
    tx.set_script(vec![0x11]);
    tx.set_witnesses(vec![]);
    assert!(
        decode::<Transaction>(&encode(&tx))
            .unwrap_err()
            .to_string()
            .contains("Witness count mismatch")
    );
    tx.set_signers(vec![]);
    assert!(
        decode::<Transaction>(&encode(&tx))
            .unwrap_err()
            .to_string()
            .contains("Signer count cannot be zero")
    );
}
#[test]
fn execution_header_cache_is_cloned_and_not_exposed() {
    let mut execution = conversions::convert_payload_header(&sample_header());
    let hash = execution.hash();
    // Preserve historical direct normative field mutation/cache behavior.
    execution.nonce += 1;
    assert_eq!(execution.hash(), hash);
    assert_eq!(execution.clone().hash(), hash);
    let fresh = conversions::convert_payload_header(&conversions::convert_ledger_header(execution));
    assert_ne!(fresh.hash(), hash);
}
