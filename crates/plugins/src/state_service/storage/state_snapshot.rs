// Copyright (C) 2015-2025 The Neo Project.
//
// state_snapshot.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{UInt160, UInt256, IStore, IStoreSnapshot, ISerializable};
use neo_core::cryptography::mpt_trie::{Trie, IStoreSnapshot as IMptStore};
use super::keys::Keys;
use super::super::network::StateRoot;
use super::super::StateServiceSettings;
use std::sync::Arc;

/// Adapter to convert IStoreSnapshot to IMptStore
struct StoreSnapshotAdapter(Arc<dyn IStoreSnapshot>);

impl IMptStore for StoreSnapshotAdapter {
    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.try_get(key).ok()
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), String> {
        self.0.put(&key, &value).map_err(|e| e.to_string())
    }

    fn delete(&self, key: Vec<u8>) -> Result<(), String> {
        self.0.delete(&key).map_err(|e| e.to_string())
    }
}

/// State snapshot implementation.
/// Matches C# StateSnapshot class exactly
pub struct StateSnapshot {
    /// Database snapshot
    /// Matches C# _snapshot field
    snapshot: Arc<dyn IStoreSnapshot>,

    /// MPT Trie - now fully functional
    /// Matches C# Trie field
    pub trie: Trie<StoreSnapshotAdapter>,
}

impl StateSnapshot {
    /// Creates a new StateSnapshot instance.
    /// Matches C# constructor
    pub fn new(store: Arc<dyn IStore>) -> Self {
        let snapshot = store.get_snapshot();
        let current_local_root_hash = Self::current_local_root_hash_static(&snapshot);
        let full_state = StateServiceSettings::default().full_state();

        // Create real MPT Trie implementation
        let adapter = Arc::new(StoreSnapshotAdapter(snapshot.clone()));
        let trie = Trie::new(adapter, current_local_root_hash, full_state);

        Self {
            snapshot,
            trie,
        }
    }
    
    /// Gets a state root by index.
    /// Matches C# GetStateRoot method
    pub fn get_state_root(&self, index: u32) -> Option<StateRoot> {
        let key = Keys::state_root(index);
        if let Ok(data) = self.snapshot.try_get(&key) {
            let mut state_root = StateRoot::new();
            if state_root.deserialize(&mut data.as_slice()).is_ok() {
                Some(state_root)
            } else {
                None
            }
        } else {
            None
        }
    }
    
    /// Adds a local state root.
    /// Matches C# AddLocalStateRoot method
    pub fn add_local_state_root(&mut self, state_root: &StateRoot) -> Result<(), String> {
        let key = Keys::state_root(state_root.index);
        let data = state_root.serialize_to_bytes()?;
        self.snapshot.put(&key, &data)?;
        
        let index_bytes = state_root.index.to_le_bytes();
        self.snapshot.put(Keys::CURRENT_LOCAL_ROOT_INDEX, &index_bytes)?;
        
        Ok(())
    }
    
    /// Gets the current local root index.
    /// Matches C# CurrentLocalRootIndex method
    pub fn current_local_root_index(&self) -> Option<u32> {
        if let Ok(bytes) = self.snapshot.try_get(Keys::CURRENT_LOCAL_ROOT_INDEX) {
            if bytes.len() == 4 {
                Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            } else {
                None
            }
        } else {
            None
        }
    }
    
    /// Gets the current local root hash (static version for construction).
    /// Matches C# CurrentLocalRootHash method
    fn current_local_root_hash_static(snapshot: &Arc<dyn IStoreSnapshot>) -> Option<UInt256> {
        if let Ok(bytes) = snapshot.try_get(Keys::CURRENT_LOCAL_ROOT_INDEX) {
            if bytes.len() == 4 {
                let index = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                let key = Keys::state_root(index);
                if let Ok(data) = snapshot.try_get(&key) {
                    let mut state_root = StateRoot::new();
                    if state_root.deserialize(&mut data.as_slice()).is_ok() {
                        return Some(state_root.root_hash);
                    }
                }
            }
        }
        None
    }

    /// Gets the current local root hash.
    /// Matches C# CurrentLocalRootHash method
    pub fn current_local_root_hash(&self) -> Option<UInt256> {
        Self::current_local_root_hash_static(&self.snapshot)
    }
    
    /// Gets the current local root hash from snapshot.
    /// Helper method for constructor
    fn current_local_root_hash(snapshot: &dyn IStoreSnapshot) -> Option<UInt256> {
        if let Ok(bytes) = snapshot.try_get(Keys::CURRENT_LOCAL_ROOT_INDEX) {
            if bytes.len() == 4 {
                let index = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                let key = Keys::state_root(index);
                if let Ok(data) = snapshot.try_get(&key) {
                    let mut state_root = StateRoot::new();
                    if state_root.deserialize(&mut data.as_slice()).is_ok() {
                        Some(state_root.root_hash)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    }
    
    /// Adds a validated state root.
    /// Matches C# AddValidatedStateRoot method
    pub fn add_validated_state_root(&mut self, state_root: &StateRoot) -> Result<(), String> {
        if state_root.witness.is_none() {
            return Err("State root witness is null".to_string());
        }
        
        let key = Keys::state_root(state_root.index);
        let data = state_root.serialize_to_bytes()?;
        self.snapshot.put(&key, &data)?;
        
        let index_bytes = state_root.index.to_le_bytes();
        self.snapshot.put(Keys::CURRENT_VALIDATED_ROOT_INDEX, &index_bytes)?;
        
        Ok(())
    }
    
    /// Gets the current validated root index.
    /// Matches C# CurrentValidatedRootIndex method
    pub fn current_validated_root_index(&self) -> Option<u32> {
        if let Ok(bytes) = self.snapshot.try_get(Keys::CURRENT_VALIDATED_ROOT_INDEX) {
            if bytes.len() == 4 {
                Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            } else {
                None
            }
        } else {
            None
        }
    }
    
    /// Gets the current validated root hash.
    /// Matches C# CurrentValidatedRootHash method
    pub fn current_validated_root_hash(&self) -> Result<UInt256, String> {
        if let Some(index) = self.current_local_root_index() {
            if let Some(state_root) = self.get_state_root(index) {
                if state_root.witness.is_some() {
                    Ok(state_root.root_hash)
                } else {
                    Err("CurrentValidatedRootHash could not get validated state root".to_string())
                }
            } else {
                Err("CurrentValidatedRootHash could not get validated state root".to_string())
            }
        } else {
            Err("CurrentValidatedRootHash could not get validated state root".to_string())
        }
    }
    
    /// Commits the snapshot.
    /// Matches C# Commit method
    pub fn commit(&mut self) -> Result<(), String> {
        self.trie.commit()?;
        self.snapshot.commit()?;
        Ok(())
    }
}

impl Drop for StateSnapshot {
    fn drop(&mut self) {
        // Resources automatically cleaned up by Rust's RAII
    }
}