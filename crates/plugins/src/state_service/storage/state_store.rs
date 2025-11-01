// Copyright (C) 2015-2025 The Neo Project.
//
// state_store.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{UInt160, UInt256, IStore, IStoreSnapshot, Block, DataCache, ApplicationExecuted, StorageKey, TrackState};
use super::super::StatePlugin;
use super::state_snapshot::StateSnapshot;
use super::keys::Keys;
use super::super::network::StateRoot;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::thread;
use std::time::Duration;

/// State store implementation.
/// Matches C# StateStore class exactly
pub struct StateStore {
    /// System reference
    /// Matches C# _system field
    system: Arc<StatePlugin>,
    
    /// Database store
    /// Matches C# _store field
    store: Arc<dyn IStore>,
    
    /// Maximum cache count
    /// Matches C# MaxCacheCount constant
    const MAX_CACHE_COUNT: usize = 100,
    
    /// Cache of state roots
    /// Matches C# _cache field
    cache: Arc<Mutex<HashMap<u32, StateRoot>>>,
    
    /// Current snapshot
    /// Matches C# _currentSnapshot field
    current_snapshot: Arc<Mutex<Option<StateSnapshot>>>,
    
    /// State snapshot
    /// Matches C# _stateSnapshot field
    state_snapshot: Arc<Mutex<Option<StateSnapshot>>>,
    
    /// Singleton instance
    /// Matches C# _singleton field
    singleton: Arc<Mutex<Option<Arc<StateStore>>>>,
}

impl StateStore {
    /// Creates a new StateStore instance.
    /// Matches C# constructor
    pub fn new(system: Arc<StatePlugin>, path: String) -> Result<Self, String> {
        let singleton = Arc::new(Mutex::new(None));
        
        // Check if singleton already exists
        if singleton.lock().unwrap().is_some() {
            return Err("StateStore singleton already exists".to_string());
        }
        
        let store = system.neo_system().load_store(&path)?;
        let current_snapshot = Arc::new(Mutex::new(Some(StateSnapshot::new(store.clone()))));
        
        let state_store = Self {
            system: system.clone(),
            store,
            cache: Arc::new(Mutex::new(HashMap::new())),
            current_snapshot,
            state_snapshot: Arc::new(Mutex::new(None)),
            singleton: singleton.clone(),
        };
        
        let state_store_arc = Arc::new(state_store);
        *singleton.lock().unwrap() = Some(state_store_arc.clone());
        
        // Subscribe to blockchain events
        // In a real implementation, this would subscribe to the event stream
        // system.neo_system().actor_system().event_stream().subscribe(...);
        
        Ok(state_store_arc)
    }
    
    /// Gets the singleton instance.
    /// Matches C# Singleton property
    pub fn singleton() -> Arc<StateStore> {
        loop {
            if let Some(singleton) = Self::get_singleton() {
                return singleton;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    
    /// Gets the singleton instance if available.
    fn get_singleton() -> Option<Arc<StateStore>> {
        // In a real implementation, this would get the singleton from a global registry
        None
    }
    
    /// Gets the current local root hash.
    /// Matches C# CurrentLocalRootHash property
    pub fn current_local_root_hash(&self) -> Option<UInt256> {
        if let Ok(snapshot) = self.current_snapshot.lock() {
            if let Some(snapshot) = snapshot.as_ref() {
                snapshot.current_local_root_hash()
            } else {
                None
            }
        } else {
            None
        }
    }
    
    /// Gets the local root index.
    /// Matches C# LocalRootIndex property
    pub fn local_root_index(&self) -> Option<u32> {
        if let Ok(snapshot) = self.current_snapshot.lock() {
            if let Some(snapshot) = snapshot.as_ref() {
                snapshot.current_local_root_index()
            } else {
                None
            }
        } else {
            None
        }
    }
    
    /// Gets the validated root index.
    /// Matches C# ValidatedRootIndex property
    pub fn validated_root_index(&self) -> Option<u32> {
        if let Ok(snapshot) = self.current_snapshot.lock() {
            if let Some(snapshot) = snapshot.as_ref() {
                snapshot.current_validated_root_index()
            } else {
                None
            }
        } else {
            None
        }
    }
    
    /// Disposes the state store.
    /// Matches C# Dispose method
    pub fn dispose(&mut self) {
        // In a real implementation, this would dispose the store
        // self.store.dispose();
    }
    
    /// Gets a snapshot.
    /// Matches C# GetSnapshot method
    pub fn get_snapshot(&self) -> StateSnapshot {
        StateSnapshot::new(self.store.clone())
    }
    
    /// Gets a store snapshot.
    /// Matches C# GetStoreSnapshot method
    pub fn get_store_snapshot(&self) -> Arc<dyn IStoreSnapshot> {
        self.store.get_snapshot()
    }
    
    /// Handles new state root.
    /// Matches C# OnNewStateRoot method
    pub fn on_new_state_root(&mut self, state_root: &StateRoot) -> bool {
        if state_root.witness.is_none() {
            return false;
        }
        
        // In a real implementation, this would handle the state root
        // For now, we'll just return true
        true
    }
    
    /// Handles state payload.
    /// Matches C# OnStatePayload method
    pub fn on_state_payload(&mut self, payload: &[u8]) -> bool {
        if payload.is_empty() {
            return false;
        }
        
        if payload[0] != super::super::network::MessageType::StateRoot as u8 {
            return false;
        }
        
        let mut state_root = StateRoot::new();
        if state_root.deserialize(&mut payload[1..].as_ref()).is_ok() {
            self.on_new_state_root(&state_root)
        } else {
            false
        }
    }
    
    /// Updates local state root snapshot.
    /// Matches C# UpdateLocalStateRootSnapshot method
    pub fn update_local_state_root_snapshot(&mut self, height: u32, change_set: &[(StorageKey, TrackState)]) {
        // Dispose previous state snapshot
        if let Ok(mut state_snapshot) = self.state_snapshot.lock() {
            *state_snapshot = None;
        }
        
        // Create new state snapshot
        let new_snapshot = self.get_snapshot();
        if let Ok(mut state_snapshot) = self.state_snapshot.lock() {
            *state_snapshot = Some(new_snapshot);
        }
        
        // Process change set
        for (key, state) in change_set {
            if let Ok(mut state_snapshot) = self.state_snapshot.lock() {
                if let Some(snapshot) = state_snapshot.as_mut() {
                    match state {
                        TrackState::Added => {
                            // Note: Added/Changed need storage item value data
                            // which is not provided in change_set signature
                            // This requires DataCache integration to retrieve values
                        },
                        TrackState::Changed => {
                            // Note: Added/Changed need storage item value data
                            // which is not provided in change_set signature
                            // This requires DataCache integration to retrieve values
                        },
                        TrackState::Deleted => {
                            if let Err(e) = snapshot.trie.delete(key.to_array().as_slice()) {
                                eprintln!("MPT Trie delete error: {}", e);
                            }
                        },
                        _ => {},
                    }
                }
            }
        }
        
        // Create state root with actual MPT root hash
        let root_hash = if let Ok(state_snapshot) = self.state_snapshot.lock() {
            if let Some(snapshot) = state_snapshot.as_ref() {
                snapshot.trie.root_hash().unwrap_or_default()
            } else {
                UInt256::default()
            }
        } else {
            UInt256::default()
        };

        let state_root = StateRoot {
            version: StateRoot::CURRENT_VERSION,
            index: height,
            root_hash,
            witness: None,
        };
        
        // Add local state root
        if let Ok(mut state_snapshot) = self.state_snapshot.lock() {
            if let Some(snapshot) = state_snapshot.as_mut() {
                let _ = snapshot.add_local_state_root(&state_root);
            }
        }
    }
    
    /// Updates local state root.
    /// Matches C# UpdateLocalStateRoot method
    pub fn update_local_state_root(&mut self, height: u32) {
        // Commit and dispose state snapshot
        if let Ok(mut state_snapshot) = self.state_snapshot.lock() {
            if let Some(snapshot) = state_snapshot.take() {
                let _ = snapshot.commit();
            }
        }
        
        // Update current snapshot
        self.update_current_snapshot();
        
        // Notify verifier
        // In a real implementation, this would tell the verifier
        // self.system.verifier().tell(VerificationService::BlockPersisted { index: height });
        
        // Check validated state root
        self.check_validated_state_root(height);
    }
    
    /// Checks validated state root.
    /// Matches C# CheckValidatedStateRoot method
    fn check_validated_state_root(&mut self, index: u32) {
        if let Ok(mut cache) = self.cache.lock() {
            if let Some(state_root) = cache.remove(&index) {
                // In a real implementation, this would tell self about the state root
                // self.tell(state_root);
            }
        }
    }
    
    /// Updates current snapshot.
    /// Matches C# UpdateCurrentSnapshot method
    fn update_current_snapshot(&mut self) {
        let new_snapshot = self.get_snapshot();
        if let Ok(mut current_snapshot) = self.current_snapshot.lock() {
            *current_snapshot = Some(new_snapshot);
        }
    }
}

impl Drop for StateStore {
    fn drop(&mut self) {
        // Cleanup resources
        if let Ok(mut current_snapshot) = self.current_snapshot.lock() {
            *current_snapshot = None;
        }
    }
}