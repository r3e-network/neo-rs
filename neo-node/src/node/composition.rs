//! Node composition root.
//!
//! This module owns the construction-time wiring between storage, blockchain,
//! network, consensus, state-root, and service subsystems. Runtime daemon flow,
//! CLI preflight, imports, and shutdown stay in the parent `node` module.

use std::path::Path;
use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::store::Store;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use super::cli::LedgerMode;
use super::config::{
    NodeConfig, open_memory_store, open_store, service_store_provider,
    validate_state_service_storage,
};
use super::context::DaemonCommitHooks;
use super::indexer_runtime;
use super::inventory_relay::{
    FAST_SYNC_BLOCK_BATCH_FLUSH_MS, FAST_SYNC_BLOCK_BATCH_SIZE, FAST_SYNC_BURST_CAPACITY,
    flush_inventory_block_batch, handle_inbound_inventory_item,
};
use super::ledger_source::{LedgerBlockSource, RpcLedgerBlockSource, store_ledger_index};
use super::observability;
use super::remote_ledger::RemoteLedgerStatus;
use super::services::{self, NodeServiceHandles, OperationalServices};
use super::sync_downloader;
use super::tasks::{TaskKind, spawn_daemon_task, spawn_daemon_task_result};
use super::workflow::RunningNode;

enum NodeLedgerBlockSource {
    Local(LedgerBlockSource<neo_storage::persistence::StoreCacheBacking<RuntimeStore>>),
    Remote(RpcLedgerBlockSource),
}

impl neo_network::BlockSource for NodeLedgerBlockSource {
    fn block_by_index(&self, index: u32) -> Option<neo_payloads::Block> {
        match self {
            Self::Local(source) => source.block_by_index(index),
            Self::Remote(source) => source.block_by_index(index),
        }
    }

    fn header_by_index(&self, index: u32) -> Option<neo_payloads::Header> {
        match self {
            Self::Local(source) => source.header_by_index(index),
            Self::Remote(source) => source.header_by_index(index),
        }
    }

    fn block_hash_by_index(&self, index: u32) -> Option<neo_primitives::UInt256> {
        match self {
            Self::Local(source) => source.block_hash_by_index(index),
            Self::Remote(source) => source.block_hash_by_index(index),
        }
    }

    fn block_by_hash(&self, hash: &neo_primitives::UInt256) -> Option<neo_payloads::Block> {
        match self {
            Self::Local(source) => source.block_by_hash(hash),
            Self::Remote(source) => source.block_by_hash(hash),
        }
    }

    fn block_index_by_hash(&self, hash: &neo_primitives::UInt256) -> Option<u32> {
        match self {
            Self::Local(source) => source.block_index_by_hash(hash),
            Self::Remote(source) => source.block_index_by_hash(hash),
        }
    }

    fn transaction_by_hash(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::Transaction> {
        match self {
            Self::Local(source) => source.transaction_by_hash(hash),
            Self::Remote(source) => source.transaction_by_hash(hash),
        }
    }

    fn extensible_by_hash(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::ExtensiblePayload> {
        match self {
            Self::Local(source) => source.extensible_by_hash(hash),
            Self::Remote(source) => source.extensible_by_hash(hash),
        }
    }

    fn contains_transaction(&self, hash: &neo_primitives::UInt256) -> bool {
        match self {
            Self::Local(source) => source.contains_transaction(hash),
            Self::Remote(source) => source.contains_transaction(hash),
        }
    }

    fn mempool_transaction_hashes(&self) -> Vec<neo_primitives::UInt256> {
        match self {
            Self::Local(source) => source.mempool_transaction_hashes(),
            Self::Remote(source) => source.mempool_transaction_hashes(),
        }
    }
}

/// Constructs the [`neo_system::Node`] with a live blockchain service
/// and a spawned [`neo_network::LocalNodeService`].
pub(in crate::node) async fn build_node(
    settings: Arc<ProtocolSettings>,
    config: &NodeConfig,
    storage_override: Option<&Path>,
    stop_at_height: Option<u32>,
    ledger_mode: LedgerMode<'_>,
    startup_bulk_import: bool,
    observability: Option<observability::ObservabilityRuntime>,
) -> anyhow::Result<RunningNode> {
    // ----- storage backend -----
    let store: Arc<RuntimeStore> = match ledger_mode {
        LedgerMode::Local => open_store(config, storage_override)?,
        LedgerMode::RemoteRpc { .. } => {
            info!(
                target: "neo::remote_ledger",
                "using ephemeral in-memory chain context; configured local ledger store will not be opened"
            );
            open_memory_store()?
        }
    };

    // Enable backend-specific fast-sync optimizations during initial catch-up
    // for higher write throughput. The node re-enables durable mode once it
    // approaches the live tip.
    let durable_tip_index = store_ledger_index(&store, false);
    let service_storage_provider = service_store_provider(config)?;
    validate_state_service_storage(
        config,
        settings.network,
        durable_tip_index,
        &service_storage_provider,
    )?;
    let durable_tip_height = durable_tip_index.unwrap_or(0);
    let use_fast_sync_store_mode = ledger_mode.uses_local_replay_services()
        && (durable_tip_height == 0 || startup_bulk_import);
    if use_fast_sync_store_mode {
        info!(
            target: "neo::sync",
            startup_bulk_import,
            durable_tip_height,
            "enabling fast-sync store mode for initial catch-up (WAL disabled, auto-compaction off)"
        );
        if store.supports_fast_sync_mode() {
            store.enable_fast_sync_mode();
        }
    }

    // Native dispatch must be available before genesis initialization, and the
    // composed Node should expose the same provider object. Build it once here
    // and hand the same Arc to every provider-aware subsystem.
    let native_contract_provider = Arc::new(neo_native_contracts::StandardNativeProvider::new());

    let OperationalServices {
        state_store,
        state_service,
        indexer_service,
        application_logs_service,
        tokens_tracker_service,
        tokens_tracker_runtime,
        durable_stores,
    } = services::build_operational_services(
        config,
        settings.network,
        ledger_mode.uses_local_replay_services(),
        use_fast_sync_store_mode,
    )?;

    let daemon_hooks = Arc::new(DaemonCommitHooks::new(
        settings.network,
        state_service.clone(),
        config.state_service.track_during_catchup,
        indexer_service.clone(),
        application_logs_service.clone(),
    ));
    let core_launch = neo_system::NodeCoreBuilder::new(
        Arc::clone(&settings),
        Arc::clone(&store),
        Arc::clone(&native_contract_provider),
        Arc::clone(&daemon_hooks),
        durable_tip_height,
    )
    .with_stop_at_height(stop_at_height)
    .build();
    let (core, blockchain_task) = core_launch.into_parts();
    let durable_tip = core.persisted_height();
    let snapshot = core.snapshot();
    let ledger_ctx = core.ledger_context();
    let mempool = core.mempool();
    let blockchain = core.blockchain();

    // A second handle on the shared snapshot serves peers' block requests, and
    // the shared mempool answers `Inv`/`Mempool`/`GetData` for unconfirmed txs.
    let mut advertised_tip = durable_tip;
    let mut remote_advertised_tip = None;
    let mut remote_tip_error = None;
    let block_source: Arc<NodeLedgerBlockSource> = match ledger_mode {
        LedgerMode::RemoteRpc { endpoint } => {
            let source = RpcLedgerBlockSource::new(endpoint.to_string(), Arc::clone(&mempool))?;
            match source.remote_tip_height() {
                Ok(height) => {
                    advertised_tip = height;
                    remote_advertised_tip = Some(height);
                }
                Err(err) => {
                    remote_tip_error = Some(err.to_string());
                    warn!(
                        target: "neo::remote_ledger",
                        endpoint,
                        error = %err,
                        "failed to read remote ledger tip height; advertising local height"
                    );
                }
            }
            info!(
                target: "neo::remote_ledger",
                endpoint,
                height = advertised_tip,
                "using remote RPC endpoint as ledger source"
            );
            Arc::new(NodeLedgerBlockSource::Remote(source))
        }
        LedgerMode::Local => Arc::new(NodeLedgerBlockSource::Local(LedgerBlockSource::new(
            Arc::clone(&snapshot),
            Arc::clone(&ledger_ctx),
            Arc::clone(&mempool),
        ))),
    };

    let shutdown = CancellationToken::new();
    let mut handles = Vec::new();
    spawn_daemon_task(
        &mut handles,
        observability.as_ref(),
        &shutdown,
        TaskKind::Essential,
        "blockchain_service",
        blockchain_task.run(),
    );

    if ledger_mode.uses_local_replay_services() {
        // C# Blockchain.OnInitialize: persist genesis on an empty store.
        blockchain
            .initialize()
            .await
            .map_err(|_| anyhow::anyhow!("blockchain service command loop closed during init"))?;
    } else {
        info!(
            target: "neo::remote_ledger",
            "local ledger initialization disabled; JSON-RPC ledger reads are delegated upstream"
        );
    }

    // ----- dBFT consensus participation -----
    // Build the validator set + this node's role from the protocol settings and
    // the `[consensus]` config. The driver itself is spawned after the network
    // exists (it needs the outbound relay handle); the inbound channel is set up
    // here so the forwarder can feed it decoded dBFT payloads.
    let consensus_enabled = config.consensus.enabled || config.consensus.auto_start;
    let consensus_setup = crate::consensus::build_consensus_setup(
        &settings,
        consensus_enabled,
        config.consensus.private_key_hex.as_deref(),
        config.consensus.hsm.as_ref(),
    )?;
    let consensus_configured = consensus_setup.is_some();
    let consensus_validators = consensus_setup
        .as_ref()
        .map(|s| Arc::new(parking_lot::RwLock::new(s.validators.clone())));
    // Validators + network magic the forwarder uses to decode/authenticate
    // inbound dBFT extensible payloads.
    let consensus_decode = consensus_setup
        .as_ref()
        .zip(consensus_validators.as_ref())
        .map(|(s, validators)| (Arc::clone(validators), s.network));
    let (consensus_inbound_tx, consensus_inbound_rx) = if consensus_configured {
        let (tx, rx) =
            tokio::sync::mpsc::channel::<neo_consensus::messages::ConsensusPayload>(1024);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    // Late-transaction feed (C# `ConsensusService.OnTransaction`): the inventory
    // forwarder pushes the hash of every freshly-accepted mempool transaction
    // here, and the consensus driver feeds it into the state machine so a backup
    // waiting on a proposal transaction can resume the round when it arrives
    // rather than view-changing on `TxNotFound`.
    let (consensus_tx_feed_tx, consensus_tx_feed_rx) = if consensus_configured {
        let (tx, rx) = tokio::sync::mpsc::channel::<neo_primitives::UInt256>(1024);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    // ----- signed StateRoot (StateValidators) participation -----
    // The driver runs whenever the state service is enabled: validators sign +
    // relay votes; observers verify + persist inbound signed roots. The inbound
    // channel is set up here so the forwarder can feed it StateService payloads;
    // the driver task is spawned after the network exists (it needs the relay).
    let state_root_setup = crate::state_root::build_state_root_setup(
        &settings,
        config.state_service.enabled && ledger_mode.uses_local_replay_services(),
        config.state_service.validator_key_hex.as_deref(),
    )?;
    let (state_root_inbound_tx, state_root_inbound_rx) = if state_root_setup.is_some() {
        let (tx, rx) = tokio::sync::mpsc::channel::<neo_payloads::ExtensiblePayload>(1024);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    // ----- inbound inventory relay: peer blocks/transactions -> ledger -----
    // The network layer is decoupled from the blockchain (C# `NeoSystem`
    // mediator), so each per-peer task forwards decoded inventory over this
    // channel; the forwarder hands blocks to the blockchain service, which
    // applies the C# `Blockchain.OnNewBlock` sequencing. The forwarder is
    // spawned *after* the network exists so it can re-announce accepted
    // transactions to peers via `Inv` (C# `LocalNode.RelayDirectly`).
    let (inv_tx, mut inv_rx) =
        tokio::sync::mpsc::channel::<neo_network::InboundInventory>(FAST_SYNC_BURST_CAPACITY);

    // ----- P2P service -----
    let channels_config = config.p2p.channels_config()?;
    let peer_registry = Arc::new(neo_network::PeerRegistry::from_config(&channels_config));
    let (net_service, network) = neo_network::LocalNodeService::with_config_and_registry(
        Arc::clone(&settings),
        channels_config,
        Arc::clone(&peer_registry),
    );
    let net_service =
        net_service.with_block_sync_mode(neo_network::BlockSyncMode::ExternalCoordinator);
    let net_service = if ledger_mode.uses_local_replay_services() {
        net_service.with_inventory_sink(inv_tx)
    } else {
        info!(
            target: "neo::remote_ledger",
            "inbound P2P inventory disabled; peer blocks and transactions will not populate local state"
        );
        net_service
    };
    let net_service = net_service.with_block_source(block_source);
    spawn_daemon_task(
        &mut handles,
        observability.as_ref(),
        &shutdown,
        TaskKind::Essential,
        "p2p_service",
        net_service.run(),
    );

    if ledger_mode.uses_local_replay_services() {
        let blockchain = blockchain.clone();
        let relay = network.clone();
        let consensus_decode = consensus_decode.clone();
        let consensus_inbound_tx = consensus_inbound_tx.clone();
        let consensus_tx_feed_tx = consensus_tx_feed_tx.clone();
        let state_root_inbound_tx = state_root_inbound_tx.clone();
        spawn_daemon_task(
            &mut handles,
            observability.as_ref(),
            &shutdown,
            TaskKind::Essential,
            "inventory_relay",
            async move {
                use tokio::time::{Duration, MissedTickBehavior};

                let mut pending_blocks: Vec<Arc<neo_payloads::Block>> =
                    Vec::with_capacity(FAST_SYNC_BLOCK_BATCH_SIZE);
                let mut flush_timer =
                    tokio::time::interval(Duration::from_millis(FAST_SYNC_BLOCK_BATCH_FLUSH_MS));
                flush_timer.set_missed_tick_behavior(MissedTickBehavior::Delay);

                loop {
                    tokio::select! {
                        maybe_item = inv_rx.recv() => {
                            let Some(item) = maybe_item else {
                                flush_inventory_block_batch(&blockchain, &mut pending_blocks).await;
                                break;
                            };
                            handle_inbound_inventory_item(
                                item,
                                &blockchain,
                                &relay,
                                &consensus_decode,
                                &consensus_inbound_tx,
                                &consensus_tx_feed_tx,
                                &state_root_inbound_tx,
                                &mut pending_blocks,
                            ).await;
                        }
                        _ = flush_timer.tick() => {
                            flush_inventory_block_batch(&blockchain, &mut pending_blocks).await;
                        }
                    }
                }
            },
        );
    } else {
        drop(inv_rx);
    }

    // ----- dBFT consensus driver -----
    // Spawn the round-driving task now that the network relay handle exists.
    // A configured key that is not in the current validator set stays idle but
    // keeps tracking imports so it can participate after a committee change.
    if let (Some(setup), Some(validators), Some(inbound_rx), Some(tx_feed_rx)) = (
        consensus_setup.filter(|_| ledger_mode.uses_local_replay_services()),
        consensus_validators,
        consensus_inbound_rx,
        consensus_tx_feed_rx,
    ) {
        // dBFT recovery-log directory: the persistent data directory (same
        // resolution as the ledger store), or `None` for an in-memory node -
        // which disables persistence (C# `DbftSettings.IgnoreRecoveryLogs`).
        let consensus_data_dir = storage_override
            .map(std::path::Path::to_path_buf)
            .or_else(|| config.storage.data_directory());
        if let Some(task) = crate::consensus::consensus_driver_task(
            setup,
            blockchain.clone(),
            Arc::clone(&mempool),
            network.clone(),
            Arc::clone(&settings),
            validators,
            Arc::clone(&store),
            consensus_data_dir.as_deref(),
            inbound_rx,
            tx_feed_rx,
        ) {
            info!(target: "neo", "dBFT consensus driver started (validator node)");
            spawn_daemon_task(
                &mut handles,
                observability.as_ref(),
                &shutdown,
                TaskKind::Essential,
                "consensus_driver",
                task,
            );
        }
    }

    // ----- signed StateRoot driver -----
    // Spawn the StateValidators vote/aggregate/verify driver now that the
    // network relay handle exists. Requires the local state store (roots to
    // attest and a home for finalized signed roots).
    if let (Some(setup), Some(inbound_rx), Some(state_store)) =
        (state_root_setup, state_root_inbound_rx, state_store.clone())
    {
        let has_key = setup.keypair.is_some();
        let task = crate::state_root::state_root_driver_task(
            setup,
            blockchain.clone(),
            network.clone(),
            Arc::clone(&settings),
            Arc::clone(&store),
            native_contract_provider.clone(),
            state_store,
            inbound_rx,
        );
        info!(
            target: "neo",
            validator = has_key,
            "signed StateRoot driver started",
        );
        spawn_daemon_task(
            &mut handles,
            observability.as_ref(),
            &shutdown,
            TaskKind::Essential,
            "state_root_driver",
            task,
        );
    }

    // ----- ledger height -> network advertisement -----
    // Seed the advertised height from the DURABLE tip before P2P sync starts,
    // so a node restarted on a populated store advertises its real height and
    // the block-sync cursor (`local_height + 1`) resumes from the persisted tip
    // instead of re-requesting the entire chain from block 1.
    let _ = network.set_block_height(advertised_tip).await;
    info!(target: "neo", height = advertised_tip, "advertised ledger tip to peers");

    // As the ledger persists blocks, advertise the new height to peers
    // (version + ping) so block-sync requests advance their cursor and
    // peers learn our progress (C# `LocalNode` reads `Ledger.CurrentIndex`).
    {
        let mut events = blockchain.subscribe();
        let network = network.clone();
        spawn_daemon_task(
            &mut handles,
            observability.as_ref(),
            &shutdown,
            TaskKind::Normal,
            "network_height_advertiser",
            async move {
                use neo_blockchain::RuntimeEvent;
                use tokio::sync::broadcast::error::RecvError;
                loop {
                    match events.recv().await {
                        Ok(RuntimeEvent::Imported { height, .. })
                        | Ok(RuntimeEvent::TipChanged { height, .. }) => {
                            let _ = network.set_block_height(height).await;
                        }
                        Ok(RuntimeEvent::Reverted { height, .. }) => {
                            let _ = network.set_block_height(height.saturating_sub(1)).await;
                        }
                        Ok(RuntimeEvent::RelayResult { .. }) => {
                            // Relay outcomes do not affect the durable chain
                            // height advertised to peers.
                        }
                        Ok(RuntimeEvent::Shutdown) => break,
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            },
        );
    }

    let remote_ledger = if let LedgerMode::RemoteRpc { endpoint } = ledger_mode {
        let status = match remote_tip_error {
            Some(error) => RemoteLedgerStatus::unavailable(endpoint.to_string(), error),
            None => RemoteLedgerStatus::new(endpoint.to_string(), remote_advertised_tip),
        };
        Some(Arc::new(status))
    } else {
        None
    };
    let service_handles = Arc::new(NodeServiceHandles::new(
        state_store.clone(),
        state_service.clone(),
        indexer_service.clone(),
        application_logs_service.clone(),
        tokens_tracker_service.clone(),
        remote_ledger,
    ));

    let node = Arc::new(
        core.into_node(network.clone())
            .map_err(|e| anyhow::anyhow!("node build failed: {e}"))?,
    );
    if ledger_mode.uses_local_replay_services() {
        spawn_daemon_task_result(
            &mut handles,
            observability.as_ref(),
            &shutdown,
            TaskKind::Normal,
            "p2p_sync_downloader",
            sync_downloader::run_coordinator_download_import(
                blockchain.clone(),
                node.sync_import_pipeline(),
                Arc::clone(&peer_registry),
                shutdown.clone(),
                sync_downloader::default_p2p_block_download_config(),
            ),
        );
    }
    if let Some((tracker_settings, tracker_store)) = tokens_tracker_runtime {
        daemon_hooks.set_tokens_tracker(Some(Arc::new(
            neo_rpc::plugins::tokens_tracker::TokensTracker::new(
                tracker_settings,
                tracker_store,
                node.settings(),
                node.native_contract_provider(),
            ),
        )));
    }

    if let Some(indexer) = indexer_service {
        // The indexer runtime is expensive during catch-up, but service
        // provider profiles must start indexing automatically once sync is
        // close enough to the peer tip. Keep one supervisor alive so cold-start
        // service nodes do not require a manual restart after initial sync.
        spawn_daemon_task(
            &mut handles,
            observability.as_ref(),
            &shutdown,
            TaskKind::Normal,
            "indexer_runtime",
            indexer_runtime::run_live_indexer_when_ready(
                blockchain.clone(),
                indexer,
                application_logs_service,
                config.indexer.backfill_on_startup,
                u64::from(durable_tip_height),
            ),
        );
    }

    Ok(RunningNode::new(
        node,
        network,
        handles,
        shutdown,
        service_handles,
        durable_stores,
    ))
}
