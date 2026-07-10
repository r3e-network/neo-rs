use super::*;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use neo_execution::ContractState;
use neo_manifest::{ContractManifest, NefFile};
use neo_primitives::UInt256;
use neo_state_service::mpt_store::MptChange;
use neo_storage::persistence::providers::{MemoryStore, RuntimeStore};
use neo_vm_rs::OpCode;

/// Contract id of the fixture contract deployed into the state trie.
pub(super) const FIXTURE_CONTRACT_ID: i32 = 77;

pub(super) struct MptFixture {
    pub(super) contract_hash: neo_primitives::UInt160,
    pub(super) root1: UInt256,
    pub(super) root2: UInt256,
    pub(super) server: RpcServer,
}

pub(super) fn fixture_storage_key(suffix: &[u8]) -> Vec<u8> {
    RpcServerState::storage_key_bytes(FIXTURE_CONTRACT_ID, suffix)
}

fn fixture_put(suffix: &[u8], value: &[u8]) -> MptChange {
    MptChange::Put {
        key: fixture_storage_key(suffix),
        value: value.to_vec(),
    }
}

/// Builds a server whose state store persists an MPT seeded across two
/// "blocks":
///
/// - block 1 deploys the fixture contract record (the
///   `ContractManagement` per-contract entry `getproof`/`getstate`
///   resolve ids through) plus three entries under prefix `0x0A` and
///   one under `0x0B`;
/// - block 2 rewrites `0x0A01`, adds `0x0A04` and deletes `0x0A02`.
pub(super) fn make_server_with_mpt(full_state: bool) -> MptFixture {
    let backing = Arc::new(RuntimeStore::Memory(MemoryStore::new()));
    let state_store =
        Arc::new(StateStore::with_mpt_store(full_state, backing).expect("state store"));
    let system = crate::server::test_support::test_system_with_services(
        ProtocolSettings::default(),
        crate::server::RpcServices::new().with_state_store(Arc::clone(&state_store)),
    );
    let mpt = state_store.mpt().expect("MPT backend enabled");

    let contract_hash =
        neo_primitives::UInt160::from_bytes(&[0x42u8; 20]).expect("fixture contract hash");
    let contract = ContractState::new(
        FIXTURE_CONTRACT_ID,
        contract_hash,
        NefFile::new("test".to_string(), vec![OpCode::PUSH1.byte()]),
        ContractManifest::new("StateFixture".to_string()),
    );
    let record = contract
        .serialize_contract_record()
        .expect("serialize fixture contract record");
    assert_eq!(
        ContractState::deserialize_contract_record(&record)
            .expect("contract record round-trips")
            .id,
        FIXTURE_CONTRACT_ID
    );

    // ContractManagement(-1) / Prefix_Contract(8) / script hash — the
    // key C# GetHistoricalContractState reads.
    let mut contract_key = (-1i32).to_le_bytes().to_vec();
    contract_key.push(8);
    contract_key.extend_from_slice(&contract_hash.to_bytes());

    let root1 = mpt
        .apply_block_changes(
            1,
            None,
            &[
                MptChange::Put {
                    key: contract_key,
                    value: record,
                },
                fixture_put(&[0x0A, 0x01], b"alpha"),
                fixture_put(&[0x0A, 0x02], b"beta"),
                fixture_put(&[0x0A, 0x03], b"gamma"),
                fixture_put(&[0x0B, 0x01], b"other-prefix"),
            ],
        )
        .expect("block 1 applies");
    let root2 = mpt
        .apply_block_changes(
            2,
            Some(root1),
            &[
                fixture_put(&[0x0A, 0x01], b"alpha-v2"),
                fixture_put(&[0x0A, 0x04], b"delta"),
                MptChange::Delete {
                    key: fixture_storage_key(&[0x0A, 0x02]),
                },
            ],
        )
        .expect("block 2 applies");

    let mut server = RpcServer::new(system, Default::default());
    server.register_handlers(RpcServerState::register_handlers());
    MptFixture {
        contract_hash,
        root1,
        root2,
        server,
    }
}

pub(super) fn b64(bytes: &[u8]) -> String {
    BASE64_STANDARD.encode(bytes)
}

pub(super) fn decode_b64_value(value: &Value) -> Vec<u8> {
    BASE64_STANDARD
        .decode(value.as_str().expect("base64 string"))
        .expect("valid base64")
}
