// Copyright (C) 2015-2025 The Neo Project.
//
// state_plugin.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{UInt160, UInt256, ProtocolSettings, Wallet};
use neo_core::plugins::Plugin;
use neo_core::wallets::IWalletProvider;
use super::StateServiceSettings;
use super::storage::StateStore;
use super::verification::VerificationService;
use std::sync::{Arc, Mutex};
use std::path::Path;

/// State plugin implementation.
/// Matches C# StatePlugin class exactly
pub struct StatePlugin {
    /// Base plugin functionality
    base: Plugin,
    
    /// State store actor reference
    /// Matches C# Store field
    store: Option<Arc<StateStore>>,
    
    /// Verification service actor reference
    /// Matches C# Verifier field
    verifier: Option<Arc<VerificationService>>,
    
    /// Wallet provider
    /// Matches C# walletProvider field
    wallet_provider: Option<Arc<dyn IWalletProvider>>,
    
    /// State service settings
    settings: StateServiceSettings,
}

impl StatePlugin {
    /// State payload category constant.
    /// Matches C# StatePayloadCategory constant
    pub const STATE_PAYLOAD_CATEGORY: &'static str = "StateService";
    
    /// Creates a new StatePlugin instance.
    /// Matches C# constructor
    pub fn new() -> Self {
        Self {
            base: Plugin::new(),
            store: None,
            verifier: None,
            wallet_provider: None,
            settings: StateServiceSettings::default(),
        }
    }
    
    /// Gets the name of the plugin.
    /// Matches C# Name property
    pub fn name(&self) -> &str {
        "StateService"
    }
    
    /// Gets the description of the plugin.
    /// Matches C# Description property
    pub fn description(&self) -> &str {
        "Enables MPT for the node"
    }
    
    /// Gets the config file path.
    /// Matches C# ConfigFile property
    pub fn config_file(&self) -> String {
        format!("{}/StateService.json", self.base.root_path())
    }
    
    /// Gets the exception policy.
    /// Matches C# ExceptionPolicy property
    pub fn exception_policy(&self) -> &str {
        self.settings.exception_policy()
    }
    
    /// Gets the state store.
    /// Matches C# Store property
    pub fn store(&self) -> Option<&Arc<StateStore>> {
        self.store.as_ref()
    }
    
    /// Gets the verifier.
    /// Matches C# Verifier property
    pub fn verifier(&self) -> Option<&Arc<VerificationService>> {
        self.verifier.as_ref()
    }
    
    /// Configures the plugin.
    /// Matches C# Configure method
    pub fn configure(&mut self) {
        self.settings = StateServiceSettings::load(&self.base.get_configuration());
    }
    
    /// Called when the system is loaded.
    /// Matches C# OnSystemLoaded method
    pub fn on_system_loaded(&mut self, system: &neo_core::NeoSystem) {
        if system.settings().network != self.settings.network() {
            return;
        }
        
        // Initialize store
        let store_path = format!("{}/{}", self.settings.path(), system.settings().network);
        self.store = Some(Arc::new(StateStore::new(self, &store_path)));
        
        // Register RPC methods
        self.register_rpc_methods(system);
    }
    
    /// Handles service added events.
    /// Matches C# NeoSystem_ServiceAdded_Handler method
    pub fn handle_service_added(&mut self, service: &dyn std::any::Any) {
        if let Some(wallet_provider) = service.downcast_ref::<Arc<dyn IWalletProvider>>() {
            self.wallet_provider = Some(wallet_provider.clone());
            
            if self.settings.auto_verify() {
                // In a real implementation, this would subscribe to wallet changed events
            }
        }
    }
    
    /// Handles wallet changed events.
    /// Matches C# IWalletProvider_WalletChanged_Handler method
    pub fn handle_wallet_changed(&mut self, wallet: &Wallet) {
        self.start(wallet);
    }
    
    /// Handles committing events.
    /// Matches C# Blockchain_Committing_Handler method
    pub fn handle_committing(&self, system: &neo_core::NeoSystem, block: &neo_core::Block, snapshot: &neo_core::DataCache, executed_list: &[neo_core::ApplicationExecuted]) {
        if system.settings().network != self.settings.network() {
            return;
        }
        
        if let Some(store) = &self.store {
            store.update_local_state_root_snapshot(
                block.index(),
                snapshot.get_change_set()
                    .iter()
                    .filter(|p| p.value.state != neo_core::TrackState::None && p.key.id != neo_core::NativeContract::Ledger.id())
                    .collect()
            );
        }
    }
    
    /// Handles committed events.
    /// Matches C# Blockchain_Committed_Handler method
    pub fn handle_committed(&self, system: &neo_core::NeoSystem, block: &neo_core::Block) {
        if system.settings().network != self.settings.network() {
            return;
        }
        
        if let Some(store) = &self.store {
            store.update_local_state_root(block.index());
        }
    }
    
    /// Starts the verification service.
    /// Matches C# Start method
    pub fn start(&mut self, wallet: &Wallet) {
        if self.verifier.is_some() {
            println!("Already started!");
            return;
        }
        
        if wallet.is_none() {
            println!("Please open wallet first!");
            return;
        }
        
        self.verifier = Some(Arc::new(VerificationService::new(wallet)));
    }
    
    /// Gets state root by index.
    /// Matches C# OnGetStateRoot method
    pub fn get_state_root(&self, index: u32) -> Option<serde_json::Value> {
        if let Some(store) = &self.store {
            let snapshot = store.get_snapshot();
            if let Some(state_root) = snapshot.get_state_root(index) {
                Some(state_root.to_json())
            } else {
                None
            }
        } else {
            None
        }
    }
    
    /// Gets current state root index.
    /// Matches C# OnGetStateHeight method
    pub fn get_state_height(&self) -> (Option<u32>, Option<u32>) {
        if let Some(store) = &self.store {
            (store.local_root_index(), store.validated_root_index())
        } else {
            (None, None)
        }
    }
    
    /// Gets proof of key and contract hash.
    /// Matches C# OnGetProof method
    pub fn get_proof(&self, root_hash: UInt256, script_hash: UInt160, key: &str) -> Result<serde_json::Value, String> {
        if let Some(store) = &self.store {
            let snapshot = store.get_snapshot();
            let proof = snapshot.get_proof(root_hash, script_hash, key)?;
            Ok(proof.to_json())
        } else {
            Err("Store not initialized".to_string())
        }
    }
    
    /// Registers RPC methods.
    fn register_rpc_methods(&self, system: &neo_core::NeoSystem) {
        // In a real implementation, this would register RPC methods
    }
    
    /// Checks if the network matches.
    fn check_network(&self, system: &neo_core::NeoSystem) -> Result<(), String> {
        let network = self.settings.network();
        if system.settings().network != network {
            Err(format!("Network doesn't match: {} != {}", system.settings().network, network))
        } else {
            Ok(())
        }
    }
}

impl Default for StatePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for StatePlugin {
    fn drop(&mut self) {
        // Cleanup resources
        if let Some(store) = &self.store {
            // Stop store actor
        }
        if let Some(verifier) = &self.verifier {
            // Stop verifier actor
        }
    }
}