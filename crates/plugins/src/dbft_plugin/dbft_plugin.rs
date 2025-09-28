// Copyright (C) 2015-2025 The Neo Project.
//
// dbft_plugin.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::dbft_plugin::consensus::consensus_service::ConsensusService;
use crate::dbft_plugin::dbft_settings::DbftSettings;
use async_trait::async_trait;
use neo_core::NeoSystem;
use neo_core::sign::{ISigner, SignerManager};
use neo_extensions::error::ExtensionResult;
use neo_extensions::plugin::{Plugin, PluginCategory, PluginContext, PluginEvent, PluginInfo};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};
use neo_core::persistence::{IStore, StoreFactory};
use neo_core::{UInt256, Transaction};

/// DBFT Plugin implementation matching C# DBFTPlugin exactly
pub struct DBFTPlugin {
    info: PluginInfo,
    settings: DbftSettings,
    neo_system: Option<Arc<NeoSystem>>,
    consensus: Option<Arc<Mutex<ConsensusService>>>,
    started: bool,
}

impl DBFTPlugin {
    /// Creates a new DBFTPlugin with settings
    /// Matches C# constructor with DbftSettings parameter
    pub fn new(settings: DbftSettings) -> Self {
        Self {
            info: PluginInfo {
                name: "DBFTPlugin".to_string(),
                version: "1.0.0".to_string(),
                description: "Consensus plugin with dBFT algorithm.".to_string(),
                author: "Neo Project".to_string(),
                dependencies: vec![],
                min_neo_version: "3.6.0".to_string(),
                category: PluginCategory::Consensus,
                priority: 0,
            },
            settings,
            neo_system: None,
            consensus: None,
            started: false,
        }
    }

    fn configure_from_path(&mut self, path: &PathBuf) -> ExtensionResult<()> {
        match fs::read_to_string(path) {
            Ok(contents) => {
                if contents.trim().is_empty() {
                    // keep defaults
                } else {
                    let value: Value = serde_json::from_str(&contents)?;
                    self.settings = DbftSettings::from_config(&value);
                }
                Ok(())
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    async fn start_with_signer(&mut self, signer: Arc<dyn ISigner>) {
        if self.started {
            return;
        }
        let Some(system) = self.neo_system.clone() else {
            warn!("DBFTPlugin: NeoSystem not available; cannot start consensus");
            return;
        };

        let service = ConsensusService::new(system, self.settings.clone(), signer);
        let svc = Arc::new(Mutex::new(service));
        self.consensus = Some(svc.clone());
        self.started = true;

        // Spawn async start task
        tokio::spawn(async move {
            let mut guard = svc.lock().await;
            guard.start().await;
        });
    }
}

#[async_trait]
impl Plugin for DBFTPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    async fn initialize(&mut self, context: &PluginContext) -> ExtensionResult<()> {
        let path = context.config_dir.join("DBFTPlugin.json");
        self.configure_from_path(&path)
    }

    async fn start(&mut self) -> ExtensionResult<()> {
        info!("DBFTPlugin ready");
        Ok(())
    }

    async fn stop(&mut self) -> ExtensionResult<()> {
        self.consensus = None;
        self.started = false;
        Ok(())
    }

    async fn handle_event(&mut self, event: &PluginEvent) -> ExtensionResult<()> {
        match event {
            PluginEvent::NodeStarted { system } => {
                // Only run on matching network
                if system.settings().network != self.settings.network {
                    self.neo_system = None;
                    return Ok(());
                }

                self.neo_system = Some(system.clone());

                if self.settings.auto_start {
                    // Start with configured signer if available
                    let signer = SignerManager::get_signer_or_default("");
                    match signer {
                        Some(signer) => self.start_with_signer(signer).await,
                        None => warn!("DBFTPlugin: no signer registered; consensus not started"),
                    }
                }

                // Attempt to inject a persistence store if the system has one registered
                // Prefer a registered store; otherwise, fallback to in-memory store for recovery state.
                let store_opt = system
                    .get_service::<Arc<dyn IStore>>("Store")
                    .ok()
                    .or_else(|| Some(StoreFactory::get_store("Memory", "")));
                if let Some(store) = store_opt {
                    if let Some(consensus) = &self.consensus {
                        let svc = consensus.clone();
                        tokio::spawn(async move {
                            svc.set_store(store).await;
                        });
                    }
                }
                Ok(())
            }
            PluginEvent::TransactionReceived { tx_hash } => {
                // Try to retrieve the transaction from the mempool and feed it to consensus
                if let Some(consensus) = &self.consensus {
                    if let Ok(hash_bytes) = hex::decode(tx_hash.trim_start_matches("0x")) {
                        if let Ok(hash) = UInt256::try_from(hash_bytes.as_slice()) {
                            // Try to get from mempool
                            let mempool = &self.neo_system.as_ref().unwrap().mem_pool;
                            if let Ok(mp) = mempool.read() {
                                if let Some(tx) = mp.try_get(&hash) {
                                    if tx.system_fee() <= self.settings.max_block_system_fee {
                                        let svc = consensus.clone();
                                        tokio::spawn(async move {
                                            let mut guard = svc.lock().await;
                                            guard.handle_transaction(tx).await;
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
            // The C# plugin filters Transaction messages at the network layer.
            // Our runtime may raise TransactionReceived separately; consensus intake
            // is wired within the node networking once available.
            _ => Ok(()),
        }
    }
}
