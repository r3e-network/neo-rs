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
use neo_core::sign::{ISigner, SignerManager};
use neo_core::NeoSystem;
use neo_extensions::error::ExtensionResult;
use neo_extensions::plugin::{
    Plugin, PluginBase, PluginCategory, PluginContext, PluginEvent, PluginInfo,
};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// DBFT Plugin implementation matching C# DBFTPlugin exactly
pub struct DBFTPlugin {
    base: PluginBase,
    settings: DbftSettings,
    neo_system: Option<Arc<NeoSystem>>,
    consensus: Option<Arc<Mutex<ConsensusService>>>,
    started: bool,
}

impl DBFTPlugin {
    /// Creates a new DBFTPlugin using default settings.
    pub fn new() -> Self {
        Self::with_settings(DbftSettings::default())
    }

    /// Creates a new DBFTPlugin with custom settings.
    pub fn with_settings(settings: DbftSettings) -> Self {
        let info = PluginInfo {
            name: "DBFTPlugin".to_string(),
            version: "1.0.0".to_string(),
            description: "Consensus plugin with dBFT algorithm.".to_string(),
            author: "Neo Project".to_string(),
            dependencies: vec![],
            min_neo_version: "3.6.0".to_string(),
            category: PluginCategory::Consensus,
            priority: 0,
        };

        Self {
            base: PluginBase::new(info),
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
        self.base.info()
    }

    async fn initialize(&mut self, context: &PluginContext) -> ExtensionResult<()> {
        if let Err(err) = self.base.ensure_directories() {
            warn!("DBFTPlugin: unable to create plugin directory: {}", err);
        }
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
                let system = match Arc::downcast::<NeoSystem>(system.clone()) {
                    Ok(system) => system,
                    Err(_) => {
                        warn!("DBFTPlugin: NodeStarted payload was not a NeoSystem instance");
                        return Ok(());
                    }
                };
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

                let store = system.store();
                if let Some(consensus) = &self.consensus {
                    let svc = consensus.clone();
                    tokio::spawn(async move {
                        let mut guard = svc.lock().await;
                        guard.set_store(store).await;
                    });
                }
                Ok(())
            }
            PluginEvent::TransactionReceived { .. } => Ok(()),
            // The C# plugin filters Transaction messages at the network layer.
            // Our runtime may raise TransactionReceived separately; consensus intake
            // is wired within the node networking once available.
            _ => Ok(()),
        }
    }
}

neo_extensions::register_plugin!(DBFTPlugin);
