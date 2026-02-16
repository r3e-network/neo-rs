// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
//
// modifications are permitted.

//! Integration tests for the Neo Core module.

use neo_core::ContainsTransactionType;
use neo_core::big_decimal::BigDecimal;
use neo_core::builders::{SignerBuilder, TransactionBuilder, WitnessBuilder};
use neo_core::events::{EventHandler, EventManager};
use neo_core::hardfork::{Hardfork, HardforkManager};
use neo_primitives::{UINT160_SIZE, UINT256_SIZE, UInt160, UInt256};

// Imports for tests moved to neo-node (Phase 2 refactoring):
// use neo_core::protocol_settings::ProtocolSettings;
// use neo_core::neo_vm::VMState;
// use neo_core::network::p2p::payloads::{Block, Header, Transaction};
// use neo_core::network::p2p::local_node::BroadcastEvent;
// use neo_core::network::p2p::RelayInventory;
// use neo_core::smart_contract::native::LedgerContract;
// use neo_core::smart_contract::trigger_type::TriggerType;
// use neo_core::state_service::StateRoot;
// use neo_core::WitnessScope;
// use neo_vm::op_code::OpCode;
// use std::net::SocketAddr;

use num_bigint::BigInt;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn to_hex(bytes: &[u8], little_endian: bool) -> String {
    let mut data = bytes.to_vec();
    if little_endian {
        data.reverse();
    }
    hex::encode(data)
}

#[test]
fn test_uint160_creation_and_comparison() {
    // Create UInt160 instance
    let mut uint1 = UInt160::new();
    uint1.value1 = 1;

    // Create UInt160 from bytes
    let mut data = [0u8; UINT160_SIZE];
    data[0] = 1;
    let uint2 = UInt160::from_bytes(&data).unwrap();

    // Compare UInt160 instances - they should be equal
    assert_eq!(uint1, uint2);

    let uint3 = UInt160::parse("0x0000000000000000000000000000000000000001").unwrap();
    let array = uint3.to_array();
    assert_eq!(array[0], 1);
    for &item in array.iter().skip(1) {
        assert_eq!(item, 0);
    }

    // Test ordering
    let mut uint4 = UInt160::new();
    uint4.value3 = 1; // Most significant part

    let mut uint5 = UInt160::new();
    uint5.value3 = 2;

    assert!(uint4 < uint5);
}

// NeoSystem moved to neo-node (Phase 2 refactoring)
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn test_persist_block_records_vm_state() {
//     let settings = ProtocolSettings::default();
//     let system = NeoSystem::new(settings, None, None).expect("NeoSystem::new should succeed");
//
//     let signer = SignerBuilder::create_empty()
//         .scope(WitnessScope::CALLED_BY_ENTRY)
//         .build();
//     let witness = WitnessBuilder::create_empty().build();
//
//     let tx = TransactionBuilder::create_empty()
//         .script(vec![OpCode::PUSH1 as u8, OpCode::RET as u8])
//         .signers(vec![signer])
//         .witnesses(vec![witness])
//         .build();
//     let tx_hash = tx.hash();
//
//     let mut header = Header::new();
//     header.set_index(0);
//     header.set_primary_index(0);
//     header.witness = WitnessBuilder::create_empty().build();
//
//     let block = Block {
//         header,
//         transactions: vec![tx.clone()],
//     };
//
//     let executed = system
//         .persist_block(block)
//         .expect("persist block should succeed");
//
//     assert_eq!(executed.len(), 3);
//     assert_eq!(executed[0].trigger, TriggerType::OnPersist);
//     assert_ne!(executed[0].vm_state, VMState::FAULT);
//     assert_eq!(executed[1].trigger, TriggerType::Application);
//     assert_eq!(executed[1].vm_state, VMState::HALT);
//     assert_eq!(executed[2].trigger, TriggerType::PostPersist);
//     assert_ne!(executed[2].vm_state, VMState::FAULT);
//
//     let executed_tx = executed[1]
//         .transaction
//         .as_ref()
//         .expect("executed transaction should be available")
//         .hash();
//     assert_eq!(executed_tx, tx_hash);
//
//     let ledger_contract = LedgerContract::new();
//     let store_cache = system.store_cache();
//     let persisted_state = ledger_contract
//         .get_transaction_state(&store_cache, &tx_hash)
//         .expect("read transaction state")
//         .expect("transaction state present");
//     assert_eq!(persisted_state.vm_state(), VMState::HALT);
// }

#[test]
fn test_uint256_creation_and_comparison() {
    // Create UInt256 instance
    let mut uint1 = UInt256::new();
    uint1.value1 = 1;

    // Create UInt256 from bytes
    let mut data = [0u8; UINT256_SIZE];
    data[0] = 1;
    let uint2 = UInt256::from_bytes(&data).unwrap();

    // Compare UInt256 instances
    assert_eq!(uint1, uint2);

    let uint3 =
        UInt256::parse("0x0000000000000000000000000000000000000000000000000000000000000001")
            .unwrap();
    let array = uint3.to_array();
    assert_eq!(array[0], 1);
    for &item in array.iter().take(UINT256_SIZE).skip(1) {
        assert_eq!(item, 0);
    }

    // Test ordering
    let mut uint4 = UInt256::new();
    uint4.value4 = 1; // Most significant part

    let mut uint5 = UInt256::new();
    uint5.value4 = 2;

    assert!(uint4 < uint5);
}

#[test]
fn test_big_decimal_operations() {
    // Create BigDecimal instances
    let bd1 = BigDecimal::new(BigInt::from(12345), 2);
    let bd2 = BigDecimal::new(BigInt::from(12345), 2);
    let bd3 = BigDecimal::new(BigInt::from(12346), 2);

    // Test equality
    assert_eq!(bd1, bd2);
    assert!(bd1 < bd3);

    // Test changing decimals
    let increased = bd1.change_decimals(4).unwrap();
    assert_eq!(increased.value(), &BigInt::from(1234500));
    assert_eq!(increased.decimals(), 4);

    // Test parsing
    let parsed = BigDecimal::parse("123.45", 2).unwrap();
    assert_eq!(parsed, bd1);

    // Test scientific notation
    let scientific = BigDecimal::parse("1.2345e2", 2).unwrap();
    assert_eq!(scientific, bd1);

    // Test formatting
    assert_eq!(bd1.to_string(), "123.45");

    // Test with trailing zeros
    let bd4 = BigDecimal::new(BigInt::from(12300), 2);
    assert_eq!(bd4.to_string(), "123");
}

#[test]
fn test_transaction_type_enum() {
    // Test ContainsTransactionType enum
    let not_exist = ContainsTransactionType::NotExist;
    let exists_in_pool = ContainsTransactionType::ExistsInPool;
    let exists_in_ledger = ContainsTransactionType::ExistsInLedger;

    // Test Display implementation
    assert_eq!(format!("{:?}", not_exist), "NotExist");
    assert_eq!(format!("{:?}", exists_in_pool), "ExistsInPool");
    assert_eq!(format!("{:?}", exists_in_ledger), "ExistsInLedger");
}

#[test]
fn test_byte_extensions() {
    // Test ByteExtensions
    let bytes = [0x01, 0x02, 0x03, 0x04];

    // Test to_hex_string
    assert_eq!(to_hex(&bytes, false), "01020304");
    assert_eq!(to_hex(&bytes, true), "04030201");
}

#[test]
fn test_uint160_extensions() {
    // Test UInt160Extensions
    let mut uint = UInt160::new();
    uint.value1 = 1;

    // Test to_array
    let array = uint.to_array();
    assert_eq!(array[0], 1);
    for &item in array.iter().take(UINT160_SIZE).skip(1) {
        assert_eq!(item, 0);
    }
}

#[test]
fn test_hardfork_manager() {
    // Test HardforkManager
    let mut manager = HardforkManager::new();

    // Register hardforks
    manager.register(Hardfork::HfAspidochelone, 100);
    manager.register(Hardfork::HfBasilisk, 200);

    // Test is_enabled
    assert!(!manager.is_enabled(Hardfork::HfAspidochelone, 99));
    assert!(manager.is_enabled(Hardfork::HfAspidochelone, 100));
    assert!(manager.is_enabled(Hardfork::HfAspidochelone, 101));

    assert!(!manager.is_enabled(Hardfork::HfBasilisk, 199));
    assert!(manager.is_enabled(Hardfork::HfBasilisk, 200));
    assert!(manager.is_enabled(Hardfork::HfBasilisk, 201));
}

struct TestHandler {
    called: Arc<AtomicBool>,
}

impl EventHandler for TestHandler {
    fn handle(&self, _sender: &dyn std::any::Any, _args: &dyn std::any::Any) {
        self.called.store(true, Ordering::SeqCst);
    }
}

#[test]
fn test_event_manager() {
    // Test EventManager
    let manager = EventManager::new();
    let called = Arc::new(AtomicBool::new(false));

    let handler = TestHandler {
        called: called.clone(),
    };

    // Register handler
    assert!(manager.register("test_event", handler));

    // Trigger event
    manager.trigger("test_event", &"sender", &"args");

    assert!(called.load(Ordering::SeqCst));
}

// NeoSystem moved to neo-node (Phase 2 refactoring)
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn test_neo_system() {
//     let settings = ProtocolSettings::default();
//     let system = NeoSystem::new(settings, None, None).expect("NeoSystem::new should succeed");
//
//     let service = Arc::new("test_service".to_string());
//     system
//         .add_named_service::<String, _>("test", service.clone())
//         .unwrap();
//     assert!(system.has_named_service("test"));
//
//     assert_eq!(
//         system.rpc_service_name(),
//         format!("RpcServer:{}", system.settings().network)
//     );
//
//     let retrieved: Arc<String> = system.get_service().unwrap().expect("service must exist");
//     assert_eq!(*retrieved, *service);
//
//     let endpoint: SocketAddr = "127.0.0.1:20333".parse().unwrap();
//     system.add_peer(endpoint, Some(20333), 0, 0, 0).unwrap();
//
//     assert_eq!(system.peer_count().await.unwrap(), 1);
//     let peers = system.peers().await.unwrap();
//     assert_eq!(peers, vec![endpoint]);
//
//     let state = system.local_node_state().await.unwrap();
//     assert_eq!(state.connected_peers_count(), 1);
//
//     assert!(system.remove_peer(endpoint).await.unwrap());
//     assert_eq!(system.peer_count().await.unwrap(), 0);
//
//     let mut tx = Transaction::new();
//     tx.set_script(vec![0x01]);
//
//     system
//         .relay_directly(RelayInventory::Transaction(tx.clone()), None)
//         .unwrap();
//     system
//         .send_directly(RelayInventory::Transaction(tx), None)
//         .unwrap();
//
//     let state = system.local_node_state().await.unwrap();
//     let history = state.broadcast_history();
//     assert_eq!(history.len(), 2);
//     assert!(matches!(history[0], BroadcastEvent::Relay(_)));
//     assert!(matches!(history[1], BroadcastEvent::Direct(_)));
//
//     system.shutdown().await.unwrap();
// }

// NeoSystem moved to neo-node (Phase 2 refactoring)
// #[tokio::test(flavor = "multi_thread")]
// async fn test_readiness_helpers() {
//     let settings = ProtocolSettings::default();
//     let system = NeoSystem::new(settings, None, None).expect("NeoSystem::new should succeed");
//
//     let status = system.readiness(Some(0));
//     assert_eq!(status.block_height, 0);
//     assert_eq!(status.header_height, 0);
//     assert_eq!(status.header_lag, 0);
//     assert!(status.healthy);
//     assert!(status.rpc_ready);
//     assert!(status.storage_ready);
//     assert!(system.is_ready(Some(0)));
//
//     let ctx = system.context();
//     assert!(ctx.is_ready(Some(0)));
//     let ctx_status = ctx.readiness(Some(0));
//     assert_eq!(ctx_status.block_height, 0);
//     assert_eq!(ctx_status.header_height, 0);
//     assert_eq!(ctx_status.header_lag, 0);
//     assert!(ctx_status.healthy);
//     assert!(ctx_status.rpc_ready);
//     assert!(ctx_status.storage_ready);
//
//     system.shutdown().await.unwrap();
// }

// NeoSystem moved to neo-node (Phase 2 refactoring)
// #[tokio::test(flavor = "multi_thread")]
// async fn test_state_store_service_registered() {
//     let settings = ProtocolSettings::default();
//     let system = NeoSystem::new_with_state_service(
//         settings,
//         None,
//         None,
//         Some(neo_core::state_service::state_store::StateServiceSettings::default()),
//     )
//     .expect("NeoSystem::new_with_state_service should succeed");
//
//     let state_store = system
//         .state_store()
//         .expect("state store lookup")
//         .expect("state store registered");
//
//     let mut snapshot = state_store.get_snapshot();
//     let root_hash = UInt256::from_bytes(&[7u8; 32]).unwrap();
//     let state_root = StateRoot::new_current(1, root_hash);
//     snapshot.add_local_state_root(&state_root).unwrap();
//     snapshot.commit().unwrap();
//     assert_eq!(state_store.local_root_index(), Some(1));
//     assert_eq!(state_store.current_local_root_hash(), Some(root_hash));
//
//     system.shutdown().await.unwrap();
// }

#[test]
fn test_builders() {
    // Test TransactionBuilder
    let tx_builder = TransactionBuilder::new();
    let _tx = tx_builder.build();

    // Test SignerBuilder
    let signer_builder = SignerBuilder::new();
    let _signer = signer_builder.build();

    // Test WitnessBuilder
    let witness_builder = WitnessBuilder::new();
    let _witness = witness_builder.build();
}

// ============================================================================
// ServiceRegistry Integration Tests (moved to neo-node, Phase 2 refactoring)
// ============================================================================

// use neo_core::neo_system::registry::ServiceRegistry;

// #[derive(Debug, Clone)]
// struct MockService {
//     name: String,
//     value: u32,
// }

// #[test]
// fn test_service_registry_typed_registration() {
//     let registry = ServiceRegistry::new();
//     let service = Arc::new(MockService {
//         name: "test".to_string(),
//         value: 42,
//     });
//
//     // Register service
//     registry
//         .register(service.clone(), Some("MockService".to_string()))
//         .expect("registration should succeed");
//
//     // Verify named lookup
//     assert!(registry.has_named_service("MockService"));
//     assert!(!registry.has_named_service("NonExistent"));
//
//     // Verify typed lookup
//     let retrieved = registry
//         .get_service::<MockService>()
//         .expect("lookup should succeed")
//         .expect("service should exist");
//
//     assert_eq!(retrieved.name, "test");
//     assert_eq!(retrieved.value, 42);
// }

// #[test]
// fn test_service_registry_named_lookup() {
//     let registry = ServiceRegistry::new();
//
//     // Register multiple services with different names
//     let service1 = Arc::new(MockService {
//         name: "first".to_string(),
//         value: 1,
//     });
//     let service2 = Arc::new(MockService {
//         name: "second".to_string(),
//         value: 2,
//     });
//
//     registry
//         .register(service1.clone(), Some("Service1".to_string()))
//         .expect("registration 1");
//     registry
//         .register(service2.clone(), Some("Service2".to_string()))
//         .expect("registration 2");
//
//     // Lookup by name
//     let retrieved1 = registry
//         .get_named_service::<MockService>("Service1")
//         .expect("lookup 1")
//         .expect("service 1 exists");
//     let retrieved2 = registry
//         .get_named_service::<MockService>("Service2")
//         .expect("lookup 2")
//         .expect("service 2 exists");
//
//     assert_eq!(retrieved1.value, 1);
//     assert_eq!(retrieved2.value, 2);
// }

// #[test]
// fn test_service_registry_concurrent_access() {
//     use std::thread;
//
//     let registry = Arc::new(ServiceRegistry::new());
//     let mut handles = vec![];
//
//     // Spawn multiple threads to register services concurrently
//     for i in 0..10 {
//         let registry_clone = Arc::clone(&registry);
//         let handle = thread::spawn(move || {
//             let service = Arc::new(MockService {
//                 name: format!("service_{}", i),
//                 value: i,
//             });
//             registry_clone
//                 .register(service, Some(format!("Service{}", i)))
//                 .expect("concurrent registration should succeed");
//         });
//         handles.push(handle);
//     }
//
//     // Wait for all threads to complete
//     for handle in handles {
//         handle.join().expect("thread should complete");
//     }
//
//     // Verify all services were registered
//     for i in 0..10 {
//         assert!(
//             registry.has_named_service(&format!("Service{}", i)),
//             "Service{} should exist",
//             i
//         );
//     }
// }
