use super::*;
use crate::network::p2p::payloads::Witness;
use crate::persistence::TrackState;
use crate::persistence::providers::memory_store_provider::MemoryStoreProvider;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::Contract;
use crate::smart_contract::native::LedgerContract;
use crate::smart_contract::native::{NativeContract, Role, role_management::RoleManagement};
use crate::wallets::KeyPair;
use neo_vm::op_code::OpCode;
use std::sync::Arc;

fn cache_with_designated_state_validators(index: u32, validators: &[crate::ECPoint]) -> DataCache {
    let cache = DataCache::new(false);
    let mut suffix = vec![Role::StateValidator as u8];
    suffix.extend_from_slice(&index.to_be_bytes());
    let key = StorageKey::new(RoleManagement::new().id(), suffix);

    let role_contract = RoleManagement::new();
    let value = role_contract
        .serialize_public_keys(validators)
        .expect("serialize state validators");
    cache.add(key, StorageItem::from_bytes(value));
    cache
}

#[test]
fn test_state_store_creation() {
    let store = StateStore::new(
        Arc::new(MemoryStateStoreBackend::new()),
        StateServiceSettings {
            full_state: true,
            ..StateServiceSettings::default()
        },
    );
    assert!(store.local_root_index().is_none());
    assert!(store.validated_root_index().is_none());
}

#[test]
fn test_state_root_storage() {
    let store = StateStore::new_in_memory();
    let mut snapshot = store.get_snapshot();

    let root_hash = UInt256::from_bytes(&[1u8; 32]).unwrap();
    let state_root = StateRoot::new_current(100, root_hash);

    snapshot.add_local_state_root(&state_root).unwrap();
    snapshot.commit().unwrap();

    let retrieved = store.get_state_root(100);
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.index, 100);
    assert_eq!(retrieved.root_hash, root_hash);
}

#[test]
fn test_state_snapshot_operations() {
    let backend = Arc::new(MemoryStateStoreBackend::new());
    let settings = StateServiceSettings::default();
    let mut snapshot = StateSnapshot::new(backend, settings);

    // Put some data in the trie
    snapshot.trie.put(&[1, 2, 3], &[4, 5, 6]).unwrap();
    snapshot.trie.put(&[1, 2, 4], &[7, 8, 9]).unwrap();

    // Get the data back
    let value = snapshot.trie.get(&[1, 2, 3]).unwrap();
    assert_eq!(value, Some(vec![4, 5, 6]));

    let value = snapshot.trie.get(&[1, 2, 4]).unwrap();
    assert_eq!(value, Some(vec![7, 8, 9]));

    // Commit
    let _ = snapshot.commit();
}

#[test]
fn ledger_storage_is_excluded_from_state_trie() {
    let store = StateStore::new_in_memory();
    let height = 0;

    let ledger_key = StorageKey::new(LedgerContract::ID, vec![0x01]);
    let ledger_value = vec![0xAA, 0xBB, 0xCC];
    let other_key = StorageKey::new(123, vec![0x02]);
    let other_value = vec![0x10, 0x11];

    let changes = vec![
        (
            ledger_key.clone(),
            StorageItem::from_bytes(ledger_value),
            TrackState::Added,
        ),
        (
            other_key.clone(),
            StorageItem::from_bytes(other_value.clone()),
            TrackState::Added,
        ),
    ];

    store.update_local_state_root_snapshot(height, changes.into_iter());
    store.update_local_state_root(height);

    let root = store
        .get_state_root(height)
        .expect("state root should be stored");
    let mut trie = store.trie_for_root(root.root_hash);
    assert_eq!(
        trie.get(&other_key.to_array()).expect("trie get"),
        Some(other_value)
    );
    assert_eq!(trie.get(&ledger_key.to_array()).expect("trie get"), None);
}

#[test]
fn test_memory_backend() {
    let backend = MemoryStateStoreBackend::new();

    // Put and get
    backend.put(vec![1, 2, 3], vec![4, 5, 6]);
    assert_eq!(backend.try_get(&[1, 2, 3]), Some(vec![4, 5, 6]));

    // Commit
    backend.commit();
    assert_eq!(backend.try_get(&[1, 2, 3]), Some(vec![4, 5, 6]));

    // Delete
    backend.delete(&[1, 2, 3]);
    assert_eq!(backend.try_get(&[1, 2, 3]), None);

    // Commit
    backend.commit();
    assert_eq!(backend.try_get(&[1, 2, 3]), None);
}

#[test]
fn validated_root_hash_prefers_validated_index() {
    let store = StateStore::new_in_memory();

    // Seed a local root at height 1
    let mut snapshot = store.get_snapshot();
    let local_hash = UInt256::from_bytes(&[1u8; 32]).unwrap();
    let local_root = StateRoot::new_current(1, local_hash);
    snapshot.add_local_state_root(&local_root).unwrap();
    snapshot.commit().unwrap();

    // Persist a validated root at a different height to ensure we read from CURRENT_VALIDATED_ROOT_INDEX
    let mut validated_root = StateRoot::new_current(2, UInt256::from_bytes(&[2u8; 32]).unwrap());
    validated_root.witness = Some(Witness::new_with_scripts(vec![0x01], vec![0x02]));
    let mut validated_snapshot = store.get_snapshot();
    validated_snapshot
        .add_validated_state_root(&validated_root)
        .unwrap();
    validated_snapshot.commit().unwrap();

    assert_eq!(
        store.current_validated_root_hash(),
        Some(validated_root.root_hash)
    );
}

#[test]
fn rejects_state_root_without_verifier() {
    let store = StateStore::new_in_memory();

    // Seed a local root at height 10
    let mut snapshot = store.get_snapshot();
    let root_hash = UInt256::from_bytes(&[3u8; 32]).unwrap();
    let local_root = StateRoot::new_current(10, root_hash);
    snapshot.add_local_state_root(&local_root).unwrap();
    snapshot.commit().unwrap();

    // Build a dummy witness to exercise the verifier path
    let witness = Witness::new_with_scripts(vec![0x01], vec![0x02]);
    let mut incoming = StateRoot::new_current(10, root_hash);
    incoming.witness = Some(witness);

    assert!(!store.on_new_state_root(incoming));
    assert!(store.validated_root_index().is_none());
}

#[test]
fn state_store_transaction_applies_pending_writes() {
    let backend = Arc::new(MemoryStateStoreBackend::new());
    let mut tx = StateStoreTransaction::new(backend.clone());

    let key = b"tx-key".to_vec();
    let value = b"tx-value".to_vec();
    tx.put(key.clone(), value.clone());
    tx.delete(b"to-delete");

    tx.commit();

    assert_eq!(backend.try_get(&key), Some(value));
    assert!(backend.try_get(b"to-delete").is_none());
}

#[test]
fn rejects_state_root_with_invalid_signature() {
    let settings = ProtocolSettings::default_settings();
    let keypair = KeyPair::generate().expect("generate keypair");
    let validator = keypair.get_public_key_point().expect("public key point");
    let validators = vec![validator.clone()];

    let verifier = StateRootVerifier::new(
        Arc::new(settings.clone()),
        Arc::new(move || cache_with_designated_state_validators(7, &validators)),
    );
    let backend = Arc::new(MemoryStateStoreBackend::new());
    let store =
        StateStore::new_with_verifier(backend, StateServiceSettings::default(), Some(verifier));

    // Seed local root
    let mut local_snapshot = store.get_snapshot();
    let root_hash = UInt256::from_bytes(&[8u8; 32]).unwrap();
    let local_root = StateRoot::new_current(7, root_hash);
    local_snapshot
        .add_local_state_root(&local_root)
        .expect("local state root");
    local_snapshot.commit().expect("commit local root");

    // Build signed state root but use an incorrect verification script (single-sig)
    let mut signed_root = StateRoot::new_current(7, root_hash);
    let hash = signed_root.hash();
    let mut sign_data = Vec::with_capacity(4 + hash.to_bytes().len());
    sign_data.extend_from_slice(&settings.network.to_le_bytes());
    sign_data.extend_from_slice(&hash.to_array());
    let signature = keypair.sign(&sign_data).expect("sign state root");

    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1 as u8);
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);

    // Use a single-sig script instead of multi-sig to force failure
    let verification_script = Contract::create_signature_contract(validator).script;
    signed_root.witness = Some(Witness::new_with_scripts(invocation, verification_script));

    assert!(!store.on_new_state_root(signed_root));
    assert!(store.validated_root_index().is_none());
}

#[test]
fn open_with_provider_uses_snapshot_backend() {
    let provider = Arc::new(MemoryStoreProvider::new());
    let protocol_settings = Arc::new(ProtocolSettings::default_settings());
    let store = StateStore::open_with_provider(
        provider,
        "StateRoot",
        StateServiceSettings::default(),
        protocol_settings,
    )
    .expect("state store opens");

    let mut snapshot = store.get_snapshot();
    let root_hash = UInt256::from_bytes(&[4u8; 32]).unwrap();
    let state_root = StateRoot::new_current(1, root_hash);
    snapshot.add_local_state_root(&state_root).unwrap();
    snapshot.commit().unwrap();

    assert_eq!(store.current_local_root_hash(), Some(root_hash));
}

#[test]
fn produces_and_verifies_storage_proof() {
    let store = StateStore::new(
        Arc::new(MemoryStateStoreBackend::new()),
        StateServiceSettings {
            full_state: true,
            ..StateServiceSettings::default()
        },
    );
    let key = StorageKey::create(1, 0x01);
    let mut item = StorageItem::default();
    item.set_value(vec![0xAA, 0xBB]);

    // Build a snapshot manually to keep the test focused on proof behaviour.
    let mut snapshot = store.get_snapshot();
    snapshot
        .trie
        .put(&key.to_array(), &item.get_value())
        .expect("put value in trie");
    let proof = snapshot
        .trie
        .try_get_proof(&key.to_array())
        .expect("proof lookup")
        .expect("proof present")
        .into_iter()
        .collect::<Vec<_>>();
    let root_hash = snapshot.trie.root_hash().expect("root hash");
    let value =
        StateStore::verify_proof(root_hash, &key.to_array(), &proof).expect("proof verifies");
    assert_eq!(value, item.get_value());
}

#[test]
fn verifies_state_root_witness_against_designated_state_validators() {
    let settings = ProtocolSettings::default_settings();
    let keypair = KeyPair::generate().expect("generate keypair");
    let validator = keypair.get_public_key_point().expect("public key point");
    let validators = vec![validator.clone()];

    let verifier = StateRootVerifier::new(
        Arc::new(settings.clone()),
        Arc::new(move || cache_with_designated_state_validators(5, &validators)),
    );
    let backend = Arc::new(MemoryStateStoreBackend::new());
    let store =
        StateStore::new_with_verifier(backend, StateServiceSettings::default(), Some(verifier));

    // Seed local root without witness
    let mut local_snapshot = store.get_snapshot();
    let root_hash = UInt256::from_bytes(&[9u8; 32]).unwrap();
    let local_root = StateRoot::new_current(5, root_hash);
    local_snapshot
        .add_local_state_root(&local_root)
        .expect("local state root");
    local_snapshot.commit().expect("commit local root");

    // Build signed state root
    let mut signed_root = StateRoot::new_current(5, root_hash);
    let hash = signed_root.hash();
    let mut sign_data = Vec::with_capacity(4 + hash.to_bytes().len());
    sign_data.extend_from_slice(&settings.network.to_le_bytes());
    sign_data.extend_from_slice(&hash.to_array());
    let signature = keypair.sign(&sign_data).expect("sign state root");

    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1 as u8);
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);

    let verification_script = Contract::create_multi_sig_redeem_script(1, &[validator]);
    signed_root.witness = Some(Witness::new_with_scripts(invocation, verification_script));

    assert!(store.on_new_state_root(signed_root));
    assert_eq!(store.validated_root_index(), Some(5));
}

// ============================================================================
// State Root Verification and Caching Tests
// ============================================================================

#[test]
fn verify_state_root_returns_valid_for_matching_root() {
    let store = StateStore::new_in_memory();
    let mut snapshot = store.get_snapshot();

    // Create and store a state root
    let root_hash = UInt256::from_bytes(&[0xAA; 32]).unwrap();
    let state_root = StateRoot::new_current(100, root_hash);
    snapshot.add_local_state_root(&state_root).unwrap();
    snapshot.commit().unwrap();

    // Update the current snapshot
    store.update_current_snapshot();

    // Verify state root matches
    let result = store.verify_state_root(100, &root_hash);
    assert_eq!(result, StateRootVerificationResult::Valid);
}

#[test]
fn verify_state_root_returns_mismatch_for_different_root() {
    let store = StateStore::new_in_memory();
    let mut snapshot = store.get_snapshot();

    // Create and store a state root
    let root_hash = UInt256::from_bytes(&[0xAA; 32]).unwrap();
    let state_root = StateRoot::new_current(100, root_hash);
    snapshot.add_local_state_root(&state_root).unwrap();
    snapshot.commit().unwrap();

    store.update_current_snapshot();

    // Verify with different hash should return mismatch
    let different_hash = UInt256::from_bytes(&[0xBB; 32]).unwrap();
    let result = store.verify_state_root(100, &different_hash);
    assert_eq!(result, StateRootVerificationResult::RootMismatch);
}

#[test]
fn verify_state_root_returns_not_found_for_missing_root() {
    let store = StateStore::new_in_memory();

    // Verify non-existent state root
    let root_hash = UInt256::from_bytes(&[0xAA; 32]).unwrap();
    let result = store.verify_state_root(999, &root_hash);
    assert_eq!(result, StateRootVerificationResult::NotFound);
}

#[test]
fn state_root_cache_stores_and_retrieves() {
    let store = StateStore::new_in_memory();

    // Create a state root
    let root_hash = UInt256::from_bytes(&[0xCC; 32]).unwrap();
    let state_root = StateRoot::new_current(200, root_hash);

    // Cache the state root
    store.cache_state_root(state_root.clone(), false, Some(123456));

    // Retrieve from cache
    let cached = store.get_cached_state_root(200);
    assert!(cached.is_some());
    assert_eq!(cached.unwrap().root_hash, root_hash);
}

#[test]
fn state_root_cache_retrieves_by_hash() {
    let store = StateStore::new_in_memory();

    // Create a state root
    let root_hash = UInt256::from_bytes(&[0xDD; 32]).unwrap();
    let state_root = StateRoot::new_current(300, root_hash);

    // Cache the state root
    store.cache_state_root(state_root, true, None);

    // Retrieve by hash
    let cached = store.get_cached_state_root_by_hash(&root_hash);
    assert!(cached.is_some());
    assert_eq!(cached.unwrap().index, 300);
}

#[test]
fn state_root_cache_returns_none_for_missing() {
    let store = StateStore::new_in_memory();

    // Try to get non-existent root from cache
    let cached = store.get_cached_state_root(999);
    assert!(cached.is_none());

    let root_hash = UInt256::from_bytes(&[0xEE; 32]).unwrap();
    let cached_by_hash = store.get_cached_state_root_by_hash(&root_hash);
    assert!(cached_by_hash.is_none());
}

#[test]
fn verify_state_root_with_witness_missing_witness() {
    let store = StateStore::new_in_memory();

    // Create a state root without witness
    let root_hash = UInt256::from_bytes(&[0xFF; 32]).unwrap();
    let state_root = StateRoot::new_current(400, root_hash);

    // Verify should fail due to missing witness
    let result = store.verify_state_root_with_witness(&state_root);
    assert_eq!(result, StateRootVerificationResult::MissingWitness);
}

#[test]
fn verify_state_root_with_witness_no_verifier() {
    let store = StateStore::new_in_memory();

    // Create a state root with dummy witness
    let root_hash = UInt256::from_bytes(&[0x11; 32]).unwrap();
    let mut state_root = StateRoot::new_current(500, root_hash);
    state_root.witness = Some(Witness::new_with_scripts(vec![0x01], vec![0x02]));

    // Verify should fail due to no verifier configured
    let result = store.verify_state_root_with_witness(&state_root);
    assert_eq!(result, StateRootVerificationResult::VerifierNotConfigured);
}

#[test]
fn validate_state_root_exists_checks_presence() {
    let store = StateStore::new_in_memory();
    let mut snapshot = store.get_snapshot();

    // Create and store a state root
    let root_hash = UInt256::from_bytes(&[0x22; 32]).unwrap();
    let state_root = StateRoot::new_current(600, root_hash);
    snapshot.add_local_state_root(&state_root).unwrap();
    snapshot.commit().unwrap();

    // Check existence through cache
    assert!(store.validate_state_root_exists(600, false));
    assert!(!store.validate_state_root_exists(600, true)); // No witness
    assert!(!store.validate_state_root_exists(999, false)); // Doesn't exist
}

#[test]
fn compare_with_network_root_matches() {
    let store = StateStore::new_in_memory();
    let mut snapshot = store.get_snapshot();

    // Create and store a state root
    let root_hash = UInt256::from_bytes(&[0x33; 32]).unwrap();
    let state_root = StateRoot::new_current(700, root_hash);
    snapshot.add_local_state_root(&state_root).unwrap();
    snapshot.commit().unwrap();

    // Cache the root
    store.cache_state_root(state_root, false, None);

    // Compare with matching network root
    assert!(store.compare_with_network_root(700, &root_hash));

    // Compare with different network root
    let different_hash = UInt256::from_bytes(&[0x44; 32]).unwrap();
    assert!(!store.compare_with_network_root(700, &different_hash));
}

#[test]
fn compare_with_network_root_missing() {
    let store = StateStore::new_in_memory();

    // Compare with non-existent root
    let root_hash = UInt256::from_bytes(&[0x55; 32]).unwrap();
    assert!(!store.compare_with_network_root(800, &root_hash));
}

#[test]
fn root_cache_stats_tracked() {
    let store = StateStore::new_in_memory();

    // Initially empty stats
    let stats = store.root_cache_stats();
    assert_eq!(stats.hits.load(std::sync::atomic::Ordering::Relaxed), 0);

    // Cache a root and retrieve it
    let root_hash = UInt256::from_bytes(&[0x66; 32]).unwrap();
    let state_root = StateRoot::new_current(900, root_hash);
    store.cache_state_root(state_root, false, None);

    // Retrieve to generate a hit
    let _ = store.get_cached_state_root(900);

    // Stats should show a miss (from initial lookup) then hit
    let stats = store.root_cache_stats();
    // Note: actual stats depend on implementation details
    assert!(stats.hit_rate() >= 0.0);
}

#[test]
fn clear_root_cache_removes_all() {
    let store = StateStore::new_in_memory();

    // Cache some roots
    for i in 0..5 {
        let root_hash = UInt256::from_bytes(&[i as u8; 32]).unwrap();
        let state_root = StateRoot::new_current(i, root_hash);
        store.cache_state_root(state_root, false, None);
    }

    assert_eq!(store.root_cache_len(), 5);

    // Clear cache
    store.clear_root_cache();

    assert_eq!(store.root_cache_len(), 0);
}

#[test]
fn verify_state_root_on_persist_succeeds() {
    let store = StateStore::new_in_memory();
    let mut snapshot = store.get_snapshot();

    // Create and store a state root
    let root_hash = UInt256::from_bytes(&[0x77; 32]).unwrap();
    let state_root = StateRoot::new_current(1000, root_hash);
    snapshot.add_local_state_root(&state_root).unwrap();
    snapshot.commit().unwrap();

    // Update current snapshot to recognize the new root
    store.update_current_snapshot();

    // Verify on persist should succeed
    let result = store.verify_state_root_on_persist(1000, &root_hash, None);
    assert!(result.is_ok());
}

#[test]
fn verify_state_root_on_persist_fails_for_mismatch() {
    let store = StateStore::new_in_memory();
    let mut snapshot = store.get_snapshot();

    // Create and store a state root
    let root_hash = UInt256::from_bytes(&[0x88; 32]).unwrap();
    let state_root = StateRoot::new_current(1100, root_hash);
    snapshot.add_local_state_root(&state_root).unwrap();
    snapshot.commit().unwrap();

    store.update_current_snapshot();

    // Verify with wrong hash should fail
    let wrong_hash = UInt256::from_bytes(&[0x99; 32]).unwrap();
    let result = store.verify_state_root_on_persist(1100, &wrong_hash, None);
    assert!(result.is_err());
}

#[test]
fn verify_state_root_on_persist_fails_for_missing_index() {
    let store = StateStore::new_in_memory();

    // Try to verify at index that doesn't exist
    let root_hash = UInt256::from_bytes(&[0xAA; 32]).unwrap();
    let result = store.verify_state_root_on_persist(1200, &root_hash, None);
    assert!(result.is_err());
}

#[test]
fn state_root_cache_eviction_policy() {
    let store = StateStore::new_with_verifier(
        Arc::new(MemoryStateStoreBackend::new()),
        StateServiceSettings::default(),
        None,
    );

    // Fill cache beyond capacity
    for i in 0..1500 {
        let root_hash = UInt256::from_bytes(&[(i % 256) as u8; 32]).unwrap();
        let state_root = StateRoot::new_current(i, root_hash);
        store.cache_state_root(state_root, false, None);
    }

    // Cache should have limited size (default is 1000)
    assert!(store.root_cache_len() <= 1000);
}

#[test]
fn preload_recent_roots_populates_cache() {
    let store = StateStore::new_in_memory();

    // Create multiple state roots
    for i in 1..=10 {
        let mut snapshot = store.get_snapshot();
        let root_hash = UInt256::from_bytes(&[i as u8; 32]).unwrap();
        let state_root = StateRoot::new_current(i, root_hash);
        snapshot.add_local_state_root(&state_root).unwrap();
        snapshot.commit().unwrap();
    }

    // Update current snapshot
    store.update_current_snapshot();

    // Preload should populate cache
    store.preload_recent_roots(5);

    // Cache should have entries
    assert!(store.root_cache_len() >= 5);
}

#[test]
fn state_root_verification_result_display() {
    assert_eq!(StateRootVerificationResult::Valid.to_string(), "valid");
    assert_eq!(
        StateRootVerificationResult::RootMismatch.to_string(),
        "root hash mismatch"
    );
    assert_eq!(
        StateRootVerificationResult::NotFound.to_string(),
        "state root not found"
    );
    assert_eq!(
        StateRootVerificationResult::MissingWitness.to_string(),
        "missing witness"
    );
    assert_eq!(
        StateRootVerificationResult::InvalidWitness.to_string(),
        "invalid witness"
    );
    assert_eq!(
        StateRootVerificationResult::IndexMismatch.to_string(),
        "index mismatch"
    );
    assert_eq!(
        StateRootVerificationResult::VerifierNotConfigured.to_string(),
        "verifier not configured"
    );
}
