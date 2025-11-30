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
use akka::{Actor, ActorContext, ActorRef, ActorResult, Cancelable, Props};
use async_trait::async_trait;
use neo_core::i_event_handlers::ITransactionAddedHandler;
use neo_core::ledger::{PersistCompleted, RelayResult, VerifyResult};
use neo_core::network::p2p::payloads::inventory_type::InventoryType;
use neo_core::network::p2p::payloads::Transaction;
use neo_core::sign::{ISigner, SignerManager};
use neo_core::NeoSystem;
use neo_core::extensions::error::ExtensionResult;
use neo_core::extensions::plugin::{
    Plugin, PluginBase, PluginCategory, PluginContext, PluginEvent, PluginInfo,
};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex as StdMutex,
};
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// DBFT Plugin implementation matching C# DBFTPlugin exactly
pub struct DBFTPlugin {
    base: PluginBase,
    settings: DbftSettings,
    neo_system: Option<Arc<NeoSystem>>,
    consensus: Option<Arc<Mutex<ConsensusService>>>,
    consensus_actor: Option<ActorRef>,
    tx_handler: Option<Arc<ConsensusTxHandler>>,
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
            consensus_actor: None,
            tx_handler: None,
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

        let service = ConsensusService::new(system.clone(), self.settings.clone(), signer);
        let svc = Arc::new(Mutex::new(service));
        let props = Props::new({
            let consensus = svc.clone();
            let sys = system.clone();
            move || ConsensusActor::new(consensus.clone(), sys.clone())
        });

        match system
            .actor_system()
            .actor_of(props, "dbft-consensus-service")
        {
            Ok(actor) => {
                let _ = actor.tell(crate::dbft_plugin::consensus::consensus_service::Start);
                self.consensus_actor = Some(actor.clone());
                self.consensus = Some(svc);
                if let Some(handler) = &self.tx_handler {
                    handler.update_actor(actor.clone());
                } else {
                    let handler = Arc::new(ConsensusTxHandler::new(actor.clone()));
                    if let Err(err) = system.register_transaction_added_handler(handler.clone()) {
                        warn!("DBFTPlugin: failed to register tx handler: {err}");
                    } else {
                        self.tx_handler = Some(handler);
                    }
                }
                self.started = true;
            }
            Err(err) => {
                warn!("DBFTPlugin: failed to start consensus actor: {err}");
            }
        }
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
        if let Some(actor) = self.consensus_actor.take() {
            let _ = actor.stop();
        }
        self.consensus = None;
        if let Some(handler) = &self.tx_handler {
            handler.deactivate();
            handler.clear_actor();
        }
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
                    if let Some(handler) = &self.tx_handler {
                        handler.deactivate();
                        handler.clear_actor();
                    }
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
                        let guard = svc.lock().await;
                        guard.set_store(store).await;
                    });
                }
                Ok(())
            }
            PluginEvent::NodeStopping => {
                if let Some(actor) = self.consensus_actor.take() {
                    let _ = actor.stop();
                }
                if let Some(handler) = &self.tx_handler {
                    handler.deactivate();
                    handler.clear_actor();
                }
                self.consensus = None;
                self.started = false;
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

impl Default for DBFTPlugin {
    fn default() -> Self {
        Self::new()
    }
}

struct ConsensusActor {
    service: Arc<Mutex<ConsensusService>>,
    neo_system: Arc<NeoSystem>,
    timer: Option<Cancelable>,
}

impl ConsensusActor {
    fn new(service: Arc<Mutex<ConsensusService>>, neo_system: Arc<NeoSystem>) -> Self {
        Self {
            service,
            neo_system,
            timer: None,
        }
    }

    async fn reschedule_timer(&mut self, ctx: &mut ActorContext) {
        if let Some(timer) = self.timer.take() {
            timer.cancel();
        }

        let (delay, height, view_number) = {
            let service = self.service.lock().await;
            let elapsed = service.clock_started.elapsed();
            let delay = service.expected_delay.saturating_sub(elapsed);
            let snapshot = service.context.try_read();
            let (height, view_number) = snapshot
                .as_ref()
                .map(|context| (context.block().index(), context.view_number()))
                .unwrap_or((0, 0));
            (delay, height, view_number)
        };

        let effective_delay = if delay.is_zero() {
            Duration::from_millis(1)
        } else {
            delay
        };

        let cancel = ctx.schedule_tell_once_cancelable(
            effective_delay,
            &ctx.self_ref(),
            crate::dbft_plugin::consensus::consensus_service::ConsensusTimer {
                height,
                view_number,
            },
            None,
        );
        self.timer = Some(cancel);
    }
}

#[async_trait]
impl Actor for ConsensusActor {
    async fn pre_start(&mut self, ctx: &mut ActorContext) -> ActorResult {
        let stream = ctx.system().event_stream();
        stream.subscribe::<RelayResult>(ctx.self_ref());
        stream.subscribe::<PersistCompleted>(ctx.self_ref());
        Ok(())
    }

    async fn post_stop(&mut self, _ctx: &mut ActorContext) -> ActorResult {
        if let Some(timer) = self.timer.take() {
            timer.cancel();
        }
        let stream = _ctx.system().event_stream();
        stream.unsubscribe::<RelayResult>(&_ctx.self_ref());
        stream.unsubscribe::<PersistCompleted>(&_ctx.self_ref());
        Ok(())
    }

    async fn handle(
        &mut self,
        message: Box<dyn std::any::Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        if message.is::<crate::dbft_plugin::consensus::consensus_service::Start>() {
            let mut guard = self.service.lock().await;
            guard.start().await;
            drop(guard);
            self.reschedule_timer(ctx).await;
            return Ok(());
        }

        if message.is::<crate::dbft_plugin::consensus::consensus_service::ConsensusTimer>() {
            let timer = *message
                .downcast::<crate::dbft_plugin::consensus::consensus_service::ConsensusTimer>()
                .expect("type check guarded by is()");
            let mut guard = self.service.lock().await;
            guard.on_timer(timer).await;
            drop(guard);
            self.reschedule_timer(ctx).await;
            return Ok(());
        }

        if message.is::<PersistCompleted>() {
            let mut guard = self.service.lock().await;
            guard.known_hashes.clear();
            guard.initialize_consensus(0).await;
            drop(guard);
            self.reschedule_timer(ctx).await;
            return Ok(());
        }

        if message.is::<Transaction>() {
            let tx = *message
                .downcast::<Transaction>()
                .expect("type check guarded by is()");
            let mut guard = self.service.lock().await;
            guard.handle_transaction(tx).await;
            drop(guard);
            self.reschedule_timer(ctx).await;
            return Ok(());
        }

        if let Ok(relay) = message.downcast::<RelayResult>() {
            if relay.result == VerifyResult::Succeed {
                match relay.inventory_type {
                    InventoryType::Extensible => {
                        let ledger = self.neo_system.context().ledger();
                        if let Some(payload) = ledger.get_extensible(&relay.hash) {
                            if payload.category == "dBFT" {
                                let mut guard = self.service.lock().await;
                                guard.on_consensus_payload(payload).await;
                                drop(guard);
                                self.reschedule_timer(ctx).await;
                            }
                        }
                    }
                    InventoryType::Transaction => {
                        let ledger = self.neo_system.context().ledger();
                        if let Some(tx) = ledger.get_transaction(&relay.hash) {
                            let mut guard = self.service.lock().await;
                            guard.handle_transaction(tx).await;
                        }
                    }
                    _ => {}
                }
            }
            return Ok(());
        }

        Ok(())
    }
}

neo_core::register_plugin!(DBFTPlugin);

struct ConsensusTxHandler {
    actor: StdMutex<Option<ActorRef>>,
    active: AtomicBool,
}

impl ITransactionAddedHandler for ConsensusTxHandler {
    fn memory_pool_transaction_added_handler(&self, _sender: &dyn std::any::Any, tx: &Transaction) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }

        if let Ok(guard) = self.actor.lock() {
            if let Some(actor) = guard.as_ref() {
                let _ = actor.tell(tx.clone());
            }
        }
    }
}

impl ConsensusTxHandler {
    fn new(actor: ActorRef) -> Self {
        Self {
            actor: StdMutex::new(Some(actor)),
            active: AtomicBool::new(true),
        }
    }

    fn update_actor(&self, actor: ActorRef) {
        if let Ok(mut guard) = self.actor.lock() {
            *guard = Some(actor);
        }
        self.active.store(true, Ordering::SeqCst);
    }

    fn deactivate(&self) {
        self.active.store(false, Ordering::SeqCst);
    }

    fn clear_actor(&self) {
        if let Ok(mut guard) = self.actor.lock() {
            guard.take();
        }
    }
}
