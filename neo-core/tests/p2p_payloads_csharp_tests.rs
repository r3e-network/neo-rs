use hex::{decode as hex_decode, encode as hex_encode};
use neo_core::cryptography::bloom_filter::BloomFilter;
use neo_core::cryptography::{ECCurve, ECPoint, NeoHash};
use neo_core::ledger::create_genesis_block;
use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable, SerializableExt};
use neo_core::network::p2p::capabilities::{NodeCapability, NodeCapabilityType};
use neo_core::network::p2p::payloads::headers_payload::MAX_HEADERS_COUNT;
use neo_core::network::p2p::payloads::inv_payload::MAX_HASHES_COUNT;
use neo_core::network::p2p::payloads::transaction::MAX_TRANSACTION_ATTRIBUTES;
use neo_core::network::p2p::payloads::{
    AddrPayload, Block, Conflicts, ExtensiblePayload, FilterAddPayload, FilterLoadPayload,
    GetBlockByIndexPayload, GetBlocksPayload, Header, HeadersPayload, InvPayload, InventoryType,
    MerkleBlockPayload, NetworkAddressWithTime, NotValidBefore, NotaryAssisted, Signer,
    Transaction, TransactionAttribute, TransactionAttributeType, VersionPayload, Witness,
    WitnessCondition, WitnessScope,
};
use neo_core::persistence::{DataCache, StorageItem, StorageKey};
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::native::{LedgerContract, NativeContract, PolicyContract};
use neo_core::{IVerifiable, UInt160, UInt256};
use neo_vm::{OpCode, VMState};
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr};

fn create_payload_genesis_block(settings: &ProtocolSettings) -> Block {
    let ledger_block = create_genesis_block(settings);
    let mut header = Header::new();
    header.set_version(ledger_block.header.version);
    header.set_prev_hash(ledger_block.header.previous_hash);
    header.set_merkle_root(ledger_block.header.merkle_root);
    header.set_timestamp(ledger_block.header.timestamp);
    header.set_nonce(ledger_block.header.nonce);
    header.set_index(ledger_block.header.index);
    header.set_primary_index(ledger_block.header.primary_index);
    header.set_next_consensus(ledger_block.header.next_consensus);
    header.witness = ledger_block
        .header
        .witnesses
        .first()
        .cloned()
        .unwrap_or_else(Witness::empty);
    Block {
        header,
        transactions: ledger_block.transactions,
    }
}

#[test]
fn csharp_ut_version_payload_size_and_roundtrip() {
    let empty = VersionPayload {
        network: 123,
        version: neo_core::network::p2p::payloads::version_payload::PROTOCOL_VERSION,
        timestamp: 456,
        nonce: 789,
        user_agent: "neo3".to_string(),
        allow_compression: true,
        capabilities: vec![],
    };
    assert_eq!(empty.size(), 22);

    let with_cap = VersionPayload {
        network: 123,
        version: neo_core::network::p2p::payloads::version_payload::PROTOCOL_VERSION,
        timestamp: 456,
        nonce: 789,
        user_agent: "neo3".to_string(),
        allow_compression: true,
        capabilities: vec![NodeCapability::tcp_server(22)],
    };
    assert_eq!(with_cap.size(), 25);

    let bytes = with_cap.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone = <VersionPayload as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(with_cap, clone);
}

#[test]
fn csharp_ut_version_payload_duplicate_capability_rejected() {
    let payload = VersionPayload {
        network: 123,
        version: neo_core::network::p2p::payloads::version_payload::PROTOCOL_VERSION,
        timestamp: 456,
        nonce: 789,
        user_agent: "neo3".to_string(),
        allow_compression: true,
        capabilities: vec![
            NodeCapability::tcp_server(22),
            NodeCapability::tcp_server(22),
        ],
    };
    let bytes = payload.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(<VersionPayload as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn csharp_ut_version_payload_allows_unknown_capabilities() {
    let unknown_a = NodeCapability::unknown_from_byte(0xFE, vec![]).expect("unknown");
    let unknown_b = NodeCapability::unknown_from_byte(0xFD, vec![0x00, 0x00]).expect("unknown");

    let payload = VersionPayload {
        network: 123,
        version: neo_core::network::p2p::payloads::version_payload::PROTOCOL_VERSION,
        timestamp: 456,
        nonce: 789,
        user_agent: "neo3".to_string(),
        allow_compression: true,
        capabilities: vec![
            NodeCapability::tcp_server(22),
            unknown_a,
            unknown_b,
            NodeCapability::full_node(1),
        ],
    };

    let bytes = payload.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone = <VersionPayload as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(clone.capabilities.len(), 4);
    assert_eq!(
        clone
            .capabilities
            .iter()
            .filter(|c| matches!(c, NodeCapability::Unknown { .. }))
            .count(),
        2
    );
}

#[test]
fn csharp_ut_network_address_with_time_size_and_endpoint() {
    let addr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let empty = NetworkAddressWithTime::new(1, addr, vec![]);
    assert_eq!(empty.size(), 21);
    assert_eq!(empty.endpoint().map(|e| e.port()).unwrap_or(0), 0);

    let with_cap = NetworkAddressWithTime::new(1, addr, vec![NodeCapability::tcp_server(22)]);
    assert_eq!(with_cap.size(), 24);
    assert_eq!(with_cap.endpoint().map(|e| e.port()).unwrap_or(0), 22);
}

#[test]
fn csharp_ut_network_address_with_time_roundtrip_and_duplicate_known_rejected() {
    let addr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let ext0 = NodeCapability::unknown(NodeCapabilityType::Extension0, vec![]).expect("unknown");

    let test = NetworkAddressWithTime::new(
        1,
        addr,
        vec![NodeCapability::tcp_server(22), ext0.clone(), ext0.clone()],
    );
    let bytes = test.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone =
        <NetworkAddressWithTime as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(test, clone);

    let dup_known = NetworkAddressWithTime::new(
        1,
        addr,
        vec![
            NodeCapability::tcp_server(22),
            NodeCapability::tcp_server(22),
        ],
    );
    let bytes = dup_known.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(<NetworkAddressWithTime as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn csharp_ut_addr_payload_size_roundtrip_and_empty_rejected() {
    let empty = AddrPayload::create(vec![]);
    assert_eq!(empty.size(), 1);

    let addr = NetworkAddressWithTime::new(1, IpAddr::V4(Ipv4Addr::UNSPECIFIED), vec![]);
    let payload = AddrPayload::create(vec![addr]);
    assert_eq!(payload.size(), 22);

    let bytes = payload.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone = <AddrPayload as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(payload, clone);

    let bytes = empty.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(<AddrPayload as Serializable>::deserialize(&mut reader).is_err());
}

fn seed_ledger_current_index(snapshot: &DataCache, index: u32) {
    const PREFIX_CURRENT_BLOCK: u8 = 12;
    let mut bytes = vec![0u8; 32];
    bytes.extend_from_slice(&index.to_le_bytes());
    let key = StorageKey::new(LedgerContract::ID, vec![PREFIX_CURRENT_BLOCK]);
    snapshot.add(key, StorageItem::from_bytes(bytes));
}

fn transaction_record_conflict_stub(block_index: u32) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    writer.write_u8(0x02).expect("write record kind");
    writer.write_u32(block_index).expect("write block index");
    writer.into_bytes()
}

fn transaction_record_full(block_index: u32, tx: &Transaction, vm_state: VMState) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    writer.write_u8(0x01).expect("write record kind");
    writer.write_u32(block_index).expect("write block index");
    writer.write_u8(vm_state as u8).expect("write vm state");

    let mut tx_writer = BinaryWriter::new();
    tx.serialize(&mut tx_writer).expect("serialize tx");
    writer
        .write_var_bytes(&tx_writer.into_bytes())
        .expect("write tx bytes");
    writer.into_bytes()
}

fn make_simple_tx() -> Transaction {
    let mut tx = Transaction::new();
    tx.set_script(vec![0x01]);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

#[test]
fn csharp_ut_get_blocks_payload_size_and_roundtrip() {
    let hash = UInt256::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff01")
        .expect("hash parse");
    let payload = GetBlocksPayload::create(hash, 5);
    assert_eq!(payload.size(), 34);

    let payload = GetBlocksPayload::create(UInt256::zero(), 1);
    assert_eq!(payload.size(), 34);

    let bytes = payload.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone = <GetBlocksPayload as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(clone.count, 1);
    assert_eq!(clone.hash_start, UInt256::zero());

    let invalid = GetBlocksPayload::create(UInt256::zero(), -2);
    let bytes = invalid.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(<GetBlocksPayload as Serializable>::deserialize(&mut reader).is_err());

    let invalid = GetBlocksPayload::create(UInt256::zero(), 0);
    let bytes = invalid.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(<GetBlocksPayload as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn csharp_ut_get_block_by_index_payload_size_and_validation() {
    let payload = GetBlockByIndexPayload::create(5, 5);
    assert_eq!(payload.size(), 6);

    let payload = GetBlockByIndexPayload::create(1, i16::MAX);
    assert_eq!(payload.size(), 6);

    let payload = GetBlockByIndexPayload::create(u32::MAX, -1);
    let bytes = payload.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone =
        <GetBlockByIndexPayload as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(clone.index_start, u32::MAX);
    assert_eq!(clone.count, -1);

    for invalid_count in [-2, 0, (MAX_HEADERS_COUNT as i16 + 1)] {
        let invalid = GetBlockByIndexPayload::create(u32::MAX, invalid_count);
        let bytes = invalid.to_array().expect("serialize");
        let mut reader = MemoryReader::new(&bytes);
        assert!(<GetBlockByIndexPayload as Serializable>::deserialize(&mut reader).is_err());
    }
}

#[test]
fn csharp_ut_headers_payload_size_and_roundtrip() {
    let empty = HeadersPayload::create(Vec::new());
    assert_eq!(empty.size(), 1);

    let header = Header::new();
    let payload = HeadersPayload::create(vec![header.clone()]);
    assert_eq!(payload.size(), 1 + header.size());

    let bytes = payload.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone = <HeadersPayload as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(clone.headers.len(), 1);
    let decoded = &clone.headers[0];
    assert_eq!(decoded.version(), header.version());
    assert_eq!(decoded.prev_hash(), header.prev_hash());
    assert_eq!(decoded.merkle_root(), header.merkle_root());
    assert_eq!(decoded.timestamp(), header.timestamp());
    assert_eq!(decoded.nonce(), header.nonce());
    assert_eq!(decoded.index(), header.index());
    assert_eq!(decoded.primary_index(), header.primary_index());
    assert_eq!(decoded.next_consensus(), header.next_consensus());
}

#[test]
fn csharp_ut_filter_load_payload_size_roundtrip_and_max_k() {
    let payload = FilterLoadPayload::new(Vec::new(), 1, u32::MAX);
    assert_eq!(payload.size(), 6);

    let filter = BloomFilter::new(8, 10, 123456).expect("bloom filter");
    let payload = FilterLoadPayload::create_from_bloom_filter(&filter);
    assert_eq!(payload.size(), 7);

    let bytes = payload.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone = <FilterLoadPayload as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(clone.filter, payload.filter);
    assert_eq!(clone.k, payload.k);
    assert_eq!(clone.tweak, payload.tweak);

    let invalid = FilterLoadPayload::new(Vec::new(), 51, u32::MAX);
    let bytes = invalid.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(<FilterLoadPayload as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn csharp_ut_filter_add_payload_size_and_roundtrip() {
    let payload = FilterAddPayload::new(Vec::new());
    assert_eq!(payload.size(), 1);

    let payload = FilterAddPayload::new(vec![1, 2, 3]);
    assert_eq!(payload.size(), 4);

    let bytes = payload.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone = <FilterAddPayload as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(clone.data, payload.data);
}

#[test]
fn csharp_ut_inv_payload_size_group_and_invalid_type() {
    let payload = InvPayload::create(InventoryType::Transaction, &[UInt256::zero()]);
    assert_eq!(payload.size(), 34);

    let hash = UInt256::parse("0x01ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00a4")
        .expect("hash parse");
    let payload = InvPayload::create(InventoryType::Transaction, &[UInt256::zero(), hash]);
    assert_eq!(payload.size(), 66);

    let mut hashes = Vec::with_capacity(MAX_HASHES_COUNT + 1);
    for index in 0..=MAX_HASHES_COUNT {
        let mut bytes = [0u8; 32];
        bytes[..4].copy_from_slice(&(index as u32).to_le_bytes());
        hashes.push(UInt256::from(bytes));
    }

    let groups = InvPayload::create_group(InventoryType::Transaction, hashes.clone());
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].inventory_type, InventoryType::Transaction);
    assert_eq!(groups[1].inventory_type, InventoryType::Transaction);
    assert_eq!(groups[0].hashes, hashes[..MAX_HASHES_COUNT].to_vec());
    assert_eq!(groups[1].hashes, hashes[MAX_HASHES_COUNT..].to_vec());

    let mut bytes = payload.to_array().expect("serialize");
    bytes[0] = 0xff;
    let mut reader = MemoryReader::new(&bytes);
    assert!(<InvPayload as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn csharp_ut_merkle_block_payload_size_and_roundtrip() {
    let settings = ProtocolSettings::default_settings();
    let mut block = create_payload_genesis_block(&settings);

    let payload = MerkleBlockPayload::create(&mut block, vec![false; 1024]);
    assert_eq!(payload.size(), 247);

    let payload = MerkleBlockPayload::create(&mut block, Vec::new());
    assert_eq!(payload.size(), 119);

    let payload = MerkleBlockPayload::create(&mut block, vec![false; 2]);
    let bytes = payload.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone =
        <MerkleBlockPayload as Serializable>::deserialize(&mut reader).expect("deserialize");

    assert_eq!(clone.tx_count, payload.tx_count);
    assert_eq!(clone.hashes, payload.hashes);
    assert_eq!(clone.flags, payload.flags);
}

#[test]
fn csharp_ut_header_hex_roundtrip() {
    const HEADER_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000000000000007227ba7b747f1a98f68679d4a98b68927646ab195a6f56b542ca5a0e6a412662493ed0e58f01000000000000000000000000000000000000000000000000000000000000000000000001000111";
    let bytes = hex_decode(HEADER_HEX).expect("hex");
    let mut reader = MemoryReader::new(&bytes);
    let header = <Header as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(header.size(), 113);

    let reserialized = header.to_array().expect("serialize");
    assert_eq!(hex_encode(reserialized), HEADER_HEX);
}

#[test]
fn csharp_ut_block_hex_roundtrip() {
    const BLOCK_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000000000000006c23be5d32679baa9c5c2aa0d329fd2a2441d7875d0f34d42f58f70428fbbbb9493ed0e58f01000000000000000000000000000000000000000000000000000000000000000000000001000111010000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000001000112010000";
    let bytes = hex_decode(BLOCK_HEX).expect("hex");
    let mut reader = MemoryReader::new(&bytes);
    let block = <Block as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(block.size(), 167);
    assert_eq!(block.transactions.len(), 1);
    assert_eq!(block.transactions[0].script(), &[0x12]);

    let reserialized = block.to_array().expect("serialize");
    assert_eq!(hex_encode(reserialized), BLOCK_HEX);
}

#[test]
fn csharp_ut_transaction_serialize_deserialize_simple() {
    const SIMPLE_HEX: &str = "000403020100e1f5050000000001000000000000000403020101000000000000000000000000000000000000000000000111010000";
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x01020304);
    tx.set_system_fee(100_000_000);
    tx.set_network_fee(1);
    tx.set_valid_until_block(0x01020304);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
    tx.set_attributes(Vec::new());
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.set_witnesses(vec![Witness::empty()]);

    let bytes = tx.to_array().expect("serialize");
    assert_eq!(hex_encode(&bytes), SIMPLE_HEX);

    let mut reader = MemoryReader::new(&bytes);
    let clone = <Transaction as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(clone.version(), 0);
    assert_eq!(clone.nonce(), 0x01020304);
    assert_eq!(clone.system_fee(), 100_000_000);
    assert_eq!(clone.network_fee(), 1);
    assert_eq!(clone.valid_until_block(), 0x01020304);
    assert_eq!(clone.signers().len(), 1);
    assert_eq!(clone.signers()[0].account, UInt160::zero());
    assert!(clone.attributes().is_empty());
    assert_eq!(clone.script(), &[OpCode::PUSH1 as u8]);
    assert_eq!(clone.witnesses().len(), 1);
    assert!(clone.witnesses()[0].invocation_script.is_empty());
    assert!(clone.witnesses()[0].verification_script.is_empty());
}

#[test]
fn csharp_ut_transaction_duplicate_signers_rejected() {
    const DUP_HEX: &str = "000403020100e1f5050000000001000000000000000403020102090807060504030201000908070605040302010080090807060504030201000908070605040302010001000111010000";
    let bytes = hex_decode(DUP_HEX).expect("hex");
    let mut reader = MemoryReader::new(&bytes);
    assert!(<Transaction as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn csharp_ut_transaction_too_many_signers_rejected() {
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x01020304);
    tx.set_system_fee(100_000_000);
    tx.set_network_fee(1);
    tx.set_valid_until_block(0x01020304);
    let mut signers = Vec::new();
    for index in 0..=MAX_TRANSACTION_ATTRIBUTES {
        let account = UInt160::from_bytes(&[index as u8; 20]).expect("account");
        signers.push(Signer::new(account, WitnessScope::CALLED_BY_ENTRY));
    }
    tx.set_signers(signers.clone());
    tx.set_attributes(Vec::new());
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.set_witnesses(vec![Witness::empty(); signers.len()]);

    let bytes = tx.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(<Transaction as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn csharp_ut_transaction_max_signers_witness_mismatch_rejected() {
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x01020304);
    tx.set_system_fee(100_000_000);
    tx.set_network_fee(1);
    tx.set_valid_until_block(0x01020304);
    let mut signers = Vec::new();
    for index in 0..MAX_TRANSACTION_ATTRIBUTES {
        let account = UInt160::from_bytes(&[index as u8; 20]).expect("account");
        signers.push(Signer::new(account, WitnessScope::CALLED_BY_ENTRY));
    }
    tx.set_signers(signers);
    tx.set_attributes(Vec::new());
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.set_witnesses(vec![Witness::empty()]);

    let bytes = tx.to_array().expect("serialize");
    assert!(!bytes.is_empty());
    let mut reader = MemoryReader::new(&bytes);
    assert!(<Transaction as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn csharp_ut_transaction_serialize_deserialize_simple_hex() {
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x01020304);
    tx.set_system_fee(100_000_000);
    tx.set_network_fee(1);
    tx.set_valid_until_block(0x01020304);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
    tx.set_attributes(Vec::new());
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.set_witnesses(vec![Witness::empty()]);

    let bytes = tx.to_array().expect("serialize");
    let hex = hex_encode(bytes);
    assert_eq!(
        hex,
        "000403020100e1f5050000000001000000000000000403020101000000000000000000000000000000000000000000000111010000"
    );

    let decoded = hex_decode(&hex).expect("hex");
    let mut reader = MemoryReader::new(&decoded);
    let clone = <Transaction as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(clone.signers().len(), 1);
    assert_eq!(clone.signers()[0].account, UInt160::zero());
    assert_eq!(clone.witnesses().len(), 1);
}

#[test]
fn csharp_ut_transaction_to_json_parity() {
    let mut script = vec![0x20; 32];
    script[0] = 0x42;

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0);
    tx.set_network_fee(0);
    tx.set_valid_until_block(0);
    tx.set_script(script);
    tx.set_system_fee(4_200_000_000);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
    tx.set_attributes(Vec::new());
    tx.set_witnesses(vec![Witness::empty()]);

    let json = tx.to_json(&ProtocolSettings::default_settings());
    assert_eq!(
        json["hash"],
        "0x0ab073429086d9e48fc87386122917989705d1c81fe4a60bf90e2fc228de3146"
    );
    assert_eq!(json["size"], 84);
    assert_eq!(json["version"], 0);
    assert!(json["attributes"].as_array().unwrap().is_empty());
    assert_eq!(json["netfee"], "0");
    assert_eq!(
        json["script"],
        "QiAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICA="
    );
    assert_eq!(json["sysfee"], "4200000000");
}

#[test]
fn csharp_ut_transaction_to_json_includes_sender_and_witnesses() {
    let settings = ProtocolSettings::default_settings();
    let account = UInt160::from_bytes(&[0x01; 20]).expect("account");
    let signer = Signer::new(account, WitnessScope::CALLED_BY_ENTRY);
    let witness = Witness::new_with_scripts(vec![0x01, 0x02], vec![0x21, 0x03]);

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(7);
    tx.set_system_fee(10);
    tx.set_network_fee(20);
    tx.set_valid_until_block(42);
    tx.set_signers(vec![signer.clone()]);
    tx.set_attributes(Vec::new());
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.set_witnesses(vec![witness.clone()]);

    let json = tx.to_json(&settings);
    let expected_sender = neo_core::wallets::Helper::to_address(&account, settings.address_version);

    assert_eq!(json["sender"], expected_sender);
    assert_eq!(json["validuntilblock"], 42);
    assert_eq!(json["signers"][0], signer.to_json());
    assert_eq!(json["witnesses"][0], witness.to_json());
}

#[test]
fn csharp_ut_transaction_to_json_sender_null_without_signers() {
    let settings = ProtocolSettings::default_settings();

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(1);
    tx.set_network_fee(0);
    tx.set_system_fee(0);
    tx.set_valid_until_block(1);
    tx.set_signers(Vec::new());
    tx.set_attributes(Vec::new());
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.set_witnesses(Vec::new());

    let json = tx.to_json(&settings);
    assert!(json["sender"].is_null());
    assert!(json["signers"].as_array().unwrap().is_empty());
    assert!(json["witnesses"].as_array().unwrap().is_empty());
}

#[test]
fn csharp_ut_signer_serialize_global_and_called_by_entry() {
    let signer = Signer::new(UInt160::zero(), WitnessScope::GLOBAL);
    let bytes = signer.to_array().expect("serialize");
    assert_eq!(
        bytes,
        hex_decode("000000000000000000000000000000000000000080").expect("hex")
    );

    let signer = Signer::new(UInt160::zero(), WitnessScope::CALLED_BY_ENTRY);
    let bytes = signer.to_array().expect("serialize");
    assert_eq!(
        bytes,
        hex_decode("000000000000000000000000000000000000000001").expect("hex")
    );
}

#[test]
fn csharp_ut_signer_serialize_custom_contracts() {
    let mut signer = Signer::new(UInt160::zero(), WitnessScope::CUSTOM_CONTRACTS);
    signer.allowed_contracts = vec![UInt160::zero()];

    let bytes = signer.to_array().expect("serialize");
    assert_eq!(
        bytes,
        hex_decode(
            "000000000000000000000000000000000000000010010000000000000000000000000000000000000000"
        )
        .expect("hex")
    );

    let mut reader = MemoryReader::new(&bytes);
    let clone = <Signer as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(clone.scopes, signer.scopes);
    assert_eq!(clone.allowed_contracts, signer.allowed_contracts);
    assert_eq!(clone.account, signer.account);
}

#[test]
fn csharp_ut_signer_serialize_custom_groups() {
    let group_bytes =
        hex_decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
            .expect("hex");
    let group = ECPoint::from_bytes_with_curve(ECCurve::secp256r1(), &group_bytes).expect("group");

    let mut signer = Signer::new(UInt160::zero(), WitnessScope::CUSTOM_GROUPS);
    signer.allowed_groups = vec![group];

    let bytes = signer.to_array().expect("serialize");
    assert_eq!(
        bytes,
        hex_decode(
            "0000000000000000000000000000000000000000200103b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c"
        )
        .expect("hex")
    );

    let mut reader = MemoryReader::new(&bytes);
    let clone = <Signer as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(clone.scopes, signer.scopes);
    assert_eq!(clone.allowed_groups, signer.allowed_groups);
    assert_eq!(clone.account, signer.account);
}

#[test]
fn csharp_ut_signer_json_cases() {
    let signer = Signer::new(UInt160::zero(), WitnessScope::GLOBAL);
    assert_eq!(
        signer.to_json().to_string(),
        "{\"account\":\"0x0000000000000000000000000000000000000000\",\"scopes\":\"Global\"}"
    );

    let signer = Signer::new(UInt160::zero(), WitnessScope::CALLED_BY_ENTRY);
    assert_eq!(
        signer.to_json().to_string(),
        "{\"account\":\"0x0000000000000000000000000000000000000000\",\"scopes\":\"CalledByEntry\"}"
    );

    let mut signer = Signer::new(UInt160::zero(), WitnessScope::CUSTOM_CONTRACTS);
    signer.allowed_contracts = vec![UInt160::zero()];
    assert_eq!(
        signer.to_json().to_string(),
        "{\"account\":\"0x0000000000000000000000000000000000000000\",\"scopes\":\"CustomContracts\",\"allowedcontracts\":[\"0x0000000000000000000000000000000000000000\"]}"
    );

    let group_bytes =
        hex_decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
            .expect("hex");
    let group = ECPoint::from_bytes_with_curve(ECCurve::secp256r1(), &group_bytes).expect("group");
    let mut signer = Signer::new(UInt160::zero(), WitnessScope::CUSTOM_GROUPS);
    signer.allowed_groups = vec![group];
    assert_eq!(
        signer.to_json().to_string(),
        "{\"account\":\"0x0000000000000000000000000000000000000000\",\"scopes\":\"CustomGroups\",\"allowedgroups\":[\"03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c\"]}"
    );
}

#[test]
fn csharp_ut_signer_json_roundtrip() {
    let json = json!({
        "account": "0x0000000000000000000000000000000000000000",
        "scopes": "CustomContracts | CustomGroups | WitnessRules",
        "allowedcontracts": ["0x0000000000000000000000000000000000000000"],
        "allowedgroups": ["03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c"],
        "rules": [{
            "action": "Allow",
            "condition": { "type": "Boolean", "expression": true }
        }]
    });

    let signer = Signer::from_json(&json).expect("from json");
    assert_eq!(signer.account, UInt160::zero());
    assert!(signer.scopes.contains(WitnessScope::CUSTOM_CONTRACTS));
    assert!(signer.scopes.contains(WitnessScope::CUSTOM_GROUPS));
    assert!(signer.scopes.contains(WitnessScope::WITNESS_RULES));
    assert_eq!(signer.allowed_contracts.len(), 1);
    assert_eq!(signer.allowed_groups.len(), 1);
    assert_eq!(signer.rules.len(), 1);
}

#[test]
fn csharp_ut_signer_equatable_semantics() {
    let group_bytes =
        hex_decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
            .expect("hex");
    let group = ECPoint::from_bytes_with_curve(ECCurve::secp256r1(), &group_bytes).expect("group");

    let rule = neo_core::WitnessRule::new(
        neo_core::WitnessRuleAction::Allow,
        WitnessCondition::Boolean { value: true },
    );

    let mut expected = Signer::new(UInt160::zero(), WitnessScope::GLOBAL);
    expected.allowed_contracts = vec![UInt160::zero()];
    expected.allowed_groups = vec![group.clone()];
    expected.rules = vec![rule.clone()];

    let mut actual = Signer::new(UInt160::zero(), WitnessScope::GLOBAL);
    actual.allowed_contracts = vec![UInt160::zero()];
    actual.allowed_groups = vec![group];
    actual.rules = vec![rule];

    let not_equal = Signer::new(UInt160::zero(), WitnessScope::WITNESS_RULES);

    assert_eq!(expected, expected);
    assert_eq!(expected, actual);
    assert_ne!(expected, not_equal);
    assert_ne!(actual, not_equal);
}

#[test]
fn csharp_ut_signer_max_nested_and_rejected_on_deserialize() {
    let condition = WitnessCondition::And {
        conditions: vec![WitnessCondition::And {
            conditions: vec![WitnessCondition::And {
                conditions: vec![WitnessCondition::Boolean { value: true }],
            }],
        }],
    };
    let rule = neo_core::WitnessRule::new(neo_core::WitnessRuleAction::Allow, condition);
    let mut signer = Signer::new(UInt160::zero(), WitnessScope::WITNESS_RULES);
    signer.rules = vec![rule];

    let bytes = signer.to_array().expect("serialize");
    assert_eq!(
        bytes,
        hex_decode("00000000000000000000000000000000000000004001010201020102010001").expect("hex")
    );

    let mut reader = MemoryReader::new(&bytes);
    assert!(<Signer as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn csharp_ut_signer_max_nested_or_rejected_on_deserialize() {
    let condition = WitnessCondition::Or {
        conditions: vec![WitnessCondition::Or {
            conditions: vec![WitnessCondition::Or {
                conditions: vec![WitnessCondition::Boolean { value: true }],
            }],
        }],
    };
    let rule = neo_core::WitnessRule::new(neo_core::WitnessRuleAction::Allow, condition);
    let mut signer = Signer::new(UInt160::zero(), WitnessScope::WITNESS_RULES);
    signer.rules = vec![rule];

    let bytes = signer.to_array().expect("serialize");
    assert_eq!(
        bytes,
        hex_decode("00000000000000000000000000000000000000004001010301030103010001").expect("hex")
    );

    let mut reader = MemoryReader::new(&bytes);
    assert!(<Signer as Serializable>::deserialize(&mut reader).is_err());
}

fn make_witness_with_lengths(invocation_len: usize, verification_len: usize) -> Witness {
    Witness::new_with_scripts(vec![0x20; invocation_len], vec![0x20; verification_len])
}

#[test]
fn csharp_ut_witness_empty_and_size() {
    let w1 = Witness::empty();
    let w2 = Witness::empty();
    assert_eq!(w1.size(), 2);
    assert_eq!(w2.size(), 2);
    assert!(w1.invocation_script.is_empty());
    assert!(w1.verification_script.is_empty());
    assert!(w2.invocation_script.is_empty());
    assert!(w2.verification_script.is_empty());
    assert!(!std::ptr::eq(&w1, &w2));
}

#[test]
fn csharp_ut_witness_size_small_and_large_arrays() {
    let small = make_witness_with_lengths(252, 253);
    assert_eq!(small.size(), 509);

    let large = make_witness_with_lengths(65_535, 65_536);
    assert_eq!(large.size(), 131_079);
}

#[test]
fn csharp_ut_witness_to_json() {
    let witness = Witness::new_with_scripts(vec![0x20, 0x20], vec![0x20, 0x20, 0x20]);
    let json = witness.to_json();
    assert_eq!(json["invocation"], "ICA=");
    assert_eq!(json["verification"], "ICAg");
}

#[test]
fn csharp_ut_witness_max_size_errors() {
    let witness = make_witness_with_lengths(1025, 10);
    let bytes = witness.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(<Witness as Serializable>::deserialize(&mut reader).is_err());

    let witness = make_witness_with_lengths(10, 1025);
    let bytes = witness.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(<Witness as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn csharp_ut_extensible_payload_size_and_roundtrip() {
    let sender = UInt160::from_bytes(&NeoHash::hash160(&[])).expect("empty script hash");
    let mut payload = ExtensiblePayload::new();
    payload.category = "123".to_string();
    payload.valid_block_start = 0;
    payload.valid_block_end = 1;
    payload.sender = sender;
    payload.data = vec![1, 2, 3];
    payload.witness = Witness::new_with_scripts(vec![3, 5, 6], Vec::new());
    assert_eq!(payload.size(), 42);

    let mut payload = ExtensiblePayload::new();
    payload.category = "123".to_string();
    payload.valid_block_start = 456;
    payload.valid_block_end = 789;
    payload.sender = sender;
    payload.data = vec![1, 2, 3];
    payload.witness = Witness::new_with_scripts(vec![0x51, 0x52, 0x53], Vec::new());
    let bytes = payload.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let mut clone =
        <ExtensiblePayload as Serializable>::deserialize(&mut reader).expect("deserialize");

    assert_eq!(clone.witness.script_hash(), sender);
    assert_eq!(clone.hash(), payload.hash());
    assert_eq!(clone.valid_block_start, 456);
    assert_eq!(clone.valid_block_end, 789);
    assert_eq!(clone.category, "123");
    assert_eq!(clone.data, vec![1, 2, 3]);

    {
        let witnesses = clone.get_witnesses();
        assert_eq!(witnesses.len(), 1);
    }
    let mut witnesses = clone.get_witnesses_mut();
    assert_eq!(witnesses.len(), 1);
    witnesses[0].invocation_script = vec![0x01];
    assert_eq!(clone.witness.invocation_script, vec![0x01]);
}

#[test]
fn csharp_ut_high_priority_attribute_parity() {
    let settings = ProtocolSettings::default_settings();
    let snapshot = DataCache::new(false);
    let mut tx = Transaction::new();

    let attr = TransactionAttribute::HighPriority;
    assert_eq!(attr.size(), 1);
    assert_eq!(attr.to_json(), json!({ "type": "HighPriority" }));

    let bytes = attr.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone = TransactionAttribute::deserialize_from(&mut reader).expect("deserialize");
    assert!(matches!(clone, TransactionAttribute::HighPriority));

    let mut invalid = bytes.clone();
    invalid[0] = 0xff;
    let mut reader = MemoryReader::new(&invalid);
    assert!(TransactionAttribute::deserialize_from(&mut reader).is_err());

    tx.set_signers(Vec::new());
    assert!(!attr.verify(&settings, &snapshot, &tx));

    let signer = Signer::new(
        UInt160::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01").expect("signer"),
        WitnessScope::GLOBAL,
    );
    tx.set_signers(vec![signer]);
    assert!(!attr.verify(&settings, &snapshot, &tx));

    let committee = neo_core::smart_contract::native::helpers::NativeHelpers::committee_address(
        &settings,
        Some(&snapshot),
    );
    tx.set_signers(vec![Signer::new(committee, WitnessScope::GLOBAL)]);
    assert!(attr.verify(&settings, &snapshot, &tx));
}

#[test]
fn csharp_ut_not_valid_before_attribute_parity() {
    let settings = ProtocolSettings::default_settings();
    let snapshot = DataCache::new(false);
    seed_ledger_current_index(&snapshot, 7);

    let attr = TransactionAttribute::NotValidBefore(NotValidBefore::new(42));
    assert_eq!(attr.size(), 5);
    assert_eq!(
        attr.to_json(),
        json!({ "type": "NotValidBefore", "height": 42 })
    );

    let bytes = attr.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone = TransactionAttribute::deserialize_from(&mut reader).expect("deserialize");
    assert!(matches!(clone, TransactionAttribute::NotValidBefore(_)));

    let mut invalid = bytes.clone();
    invalid[0] = 0xff;
    let mut reader = MemoryReader::new(&invalid);
    assert!(TransactionAttribute::deserialize_from(&mut reader).is_err());

    let tx = Transaction::new();
    let mut attr = NotValidBefore::new(8);
    assert!(!attr.verify(&settings, &snapshot, &tx));
    attr.height = 7;
    assert!(attr.verify(&settings, &snapshot, &tx));
}

#[test]
fn csharp_ut_conflicts_attribute_parity() {
    const PREFIX_TRANSACTION: u8 = 11;
    let settings = ProtocolSettings::default_settings();
    let snapshot = DataCache::new(false);
    let hash = UInt256::from([1u8; 32]);
    let key = StorageKey::create_with_uint256(LedgerContract::ID, PREFIX_TRANSACTION, &hash);

    let attr = TransactionAttribute::Conflicts(Conflicts::new(hash));
    assert_eq!(attr.size(), 33);
    assert_eq!(
        attr.to_json(),
        json!({
            "type": "Conflicts",
            "hash": "0x0101010101010101010101010101010101010101010101010101010101010101"
        })
    );

    let bytes = attr.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone = TransactionAttribute::deserialize_from(&mut reader).expect("deserialize");
    assert!(matches!(clone, TransactionAttribute::Conflicts(_)));

    let mut invalid = bytes.clone();
    invalid[0] = 0xff;
    let mut reader = MemoryReader::new(&invalid);
    assert!(TransactionAttribute::deserialize_from(&mut reader).is_err());

    let tx = make_simple_tx();

    snapshot.add(
        key.clone(),
        StorageItem::from_bytes(transaction_record_conflict_stub(0)),
    );
    assert!(attr.verify(&settings, &snapshot, &tx));

    snapshot.delete(&key);
    let record = transaction_record_full(123, &tx, VMState::NONE);
    snapshot.add(key.clone(), StorageItem::from_bytes(record));
    assert!(!attr.verify(&settings, &snapshot, &tx));

    snapshot.delete(&key);
    assert!(attr.verify(&settings, &snapshot, &tx));
}

#[test]
fn csharp_ut_notary_assisted_attribute_parity() {
    const PREFIX_ATTRIBUTE_FEE: u8 = 20;
    let settings = ProtocolSettings::default_settings();
    let snapshot = DataCache::new(false);

    let notary_hash =
        neo_core::smart_contract::Helper::get_contract_hash(&UInt160::zero(), 0, "Notary");
    let expected =
        UInt160::parse("0xc1e14f19c3e60d0b9244d06dd7ba9b113135ec3b").expect("notary hash");
    assert_eq!(notary_hash, expected);

    let attr = TransactionAttribute::NotaryAssisted(NotaryAssisted::new(4));
    assert_eq!(attr.size(), 2);
    assert_eq!(
        attr.to_json(),
        json!({ "type": "NotaryAssisted", "nkeys": 4 })
    );

    let bytes = attr.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone = TransactionAttribute::deserialize_from(&mut reader).expect("deserialize");
    assert!(matches!(clone, TransactionAttribute::NotaryAssisted(_)));

    let mut invalid = bytes.clone();
    invalid[0] = 0xff;
    let mut reader = MemoryReader::new(&invalid);
    assert!(TransactionAttribute::deserialize_from(&mut reader).is_err());

    let mut tx_good = Transaction::new();
    tx_good.set_signers(vec![
        Signer::new(notary_hash, WitnessScope::GLOBAL),
        Signer::new(UInt160::zero(), WitnessScope::GLOBAL),
    ]);
    let tx_bad1 = {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(notary_hash, WitnessScope::GLOBAL)]);
        tx
    };
    let tx_bad2 = {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(
            UInt160::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01").expect("signer"),
            WitnessScope::GLOBAL,
        )]);
        tx
    };

    if let TransactionAttribute::NotaryAssisted(attr) = &attr {
        assert!(attr.verify(&settings, &snapshot, &tx_good));
        assert!(!attr.verify(&settings, &snapshot, &tx_bad1));
        assert!(!attr.verify(&settings, &snapshot, &tx_bad2));
    } else {
        panic!("expected NotaryAssisted attribute");
    }

    let fee_key = StorageKey::create_with_byte(
        PolicyContract::new().id(),
        PREFIX_ATTRIBUTE_FEE,
        TransactionAttributeType::NotaryAssisted as u8,
    );
    snapshot.add(
        fee_key,
        StorageItem::from_bytes(
            PolicyContract::DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE
                .to_le_bytes()
                .to_vec(),
        ),
    );

    let fee = attr.calculate_network_fee(&snapshot, &tx_bad1);
    assert_eq!(
        fee,
        (4 + 1) * PolicyContract::DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE as i64
    );
}
