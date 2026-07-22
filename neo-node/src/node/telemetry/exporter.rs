//! Prometheus metrics registration, refresh, and rendering.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use hyper::{Body, Response};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::persistence::{MdbxEnvironmentInfo, TransactionalStore};
use prometheus::{Encoder, Gauge, IntGauge, IntGaugeVec, Opts, Registry, TextEncoder};
use tracing::warn;

use super::super::observability::observability_ledger_height;
use super::super::remote_ledger::RemoteLedgerStatus;
use super::super::services::NodeServiceHandles;
use super::readiness::{ReadinessSnapshot, indexer_readiness, readiness_response};

pub(super) struct MetricsExporter<
    P = neo_native_contracts::StandardNativeProvider,
    S = neo_storage::persistence::providers::MemoryStore,
> where
    P: NativeContractProvider,
    S: TransactionalStore,
{
    registry: Registry,
    node: Arc<neo_system::Node<P, S>>,
    services: Arc<NodeServiceHandles<S>>,
    started_at: Instant,
    up: IntGauge,
    info: IntGaugeVec,
    uptime_seconds: Gauge,
    ledger_height: IntGauge,
    connected_peers: IntGauge,
    mempool_transactions: IntGauge,
    mempool_verified_transactions: IntGauge,
    mempool_unverified_transactions: IntGauge,
    header_cache_entries: IntGauge,
    service_enabled: IntGaugeVec,
    indexer_up: IntGauge,
    indexer_indexed_height: IntGauge,
    indexer_indexed_blocks: IntGauge,
    indexer_indexed_transactions: IntGauge,
    indexer_indexed_accounts: IntGauge,
    indexer_indexed_notifications: IntGauge,
    indexer_indexed_notification_accounts: IntGauge,
    indexer_blocks_behind: IntGauge,
    indexer_synced: IntGauge,
    network_label: String,
}

impl<P, S> MetricsExporter<P, S>
where
    P: NativeContractProvider + 'static,
    S: TransactionalStore + 'static,
{
    pub(super) fn new(
        node: Arc<neo_system::Node<P, S>>,
        services: Arc<NodeServiceHandles<S>>,
    ) -> anyhow::Result<Self> {
        let registry = Registry::new();
        let up = IntGauge::new("neo_node_up", "Whether the node process is running")?;
        let info = IntGaugeVec::new(
            Opts::new("neo_node_info", "Static neo-node build and network labels"),
            &["version", "network"],
        )?;
        let uptime_seconds = Gauge::new(
            "neo_node_uptime_seconds",
            "Seconds since the metrics exporter was started",
        )?;
        let ledger_height = IntGauge::new(
            "neo_node_ledger_height",
            "Current persisted ledger height from the Ledger native contract",
        )?;
        let connected_peers = IntGauge::new(
            "neo_node_connected_peers",
            "Connected peers folded from network events",
        )?;
        let mempool_transactions = IntGauge::new(
            "neo_node_mempool_transactions",
            "Total transactions currently held in the memory pool",
        )?;
        let mempool_verified_transactions = IntGauge::new(
            "neo_node_mempool_verified_transactions",
            "Verified transactions currently held in the memory pool",
        )?;
        let mempool_unverified_transactions = IntGauge::new(
            "neo_node_mempool_unverified_transactions",
            "Unverified transactions currently held in the memory pool",
        )?;
        let header_cache_entries = IntGauge::new(
            "neo_node_header_cache_entries",
            "Headers currently retained ahead of the persisted tip",
        )?;
        let service_enabled = IntGaugeVec::new(
            Opts::new(
                "neo_node_service_enabled",
                "Whether an optional node service is registered in the runtime",
            ),
            &["service"],
        )?;
        let indexer_up = IntGauge::new(
            "neo_node_indexer_up",
            "Whether the registered NeoIndexer service can report status",
        )?;
        let indexer_indexed_height = IntGauge::new(
            "neo_node_indexer_indexed_height",
            "Highest block height indexed by NeoIndexer, or -1 when unavailable",
        )?;
        let indexer_indexed_blocks = IntGauge::new(
            "neo_node_indexer_indexed_blocks",
            "Number of canonical blocks indexed by NeoIndexer",
        )?;
        let indexer_indexed_transactions = IntGauge::new(
            "neo_node_indexer_indexed_transactions",
            "Number of transactions indexed by NeoIndexer",
        )?;
        let indexer_indexed_accounts = IntGauge::new(
            "neo_node_indexer_indexed_accounts",
            "Number of signer accounts indexed by NeoIndexer",
        )?;
        let indexer_indexed_notifications = IntGauge::new(
            "neo_node_indexer_indexed_notifications",
            "Number of smart-contract notifications indexed by NeoIndexer",
        )?;
        let indexer_indexed_notification_accounts = IntGauge::new(
            "neo_node_indexer_indexed_notification_accounts",
            "Number of notification participant accounts indexed by NeoIndexer",
        )?;
        let indexer_blocks_behind = IntGauge::new(
            "neo_node_indexer_blocks_behind",
            "Difference between current ledger height and NeoIndexer indexed height, or -1 when unavailable",
        )?;
        let indexer_synced = IntGauge::new(
            "neo_node_indexer_synced",
            "Whether NeoIndexer indexed height exactly matches the current ledger height",
        )?;

        registry.register(Box::new(up.clone()))?;
        registry.register(Box::new(info.clone()))?;
        registry.register(Box::new(uptime_seconds.clone()))?;
        registry.register(Box::new(ledger_height.clone()))?;
        registry.register(Box::new(connected_peers.clone()))?;
        registry.register(Box::new(mempool_transactions.clone()))?;
        registry.register(Box::new(mempool_verified_transactions.clone()))?;
        registry.register(Box::new(mempool_unverified_transactions.clone()))?;
        registry.register(Box::new(header_cache_entries.clone()))?;
        registry.register(Box::new(service_enabled.clone()))?;
        registry.register(Box::new(indexer_up.clone()))?;
        registry.register(Box::new(indexer_indexed_height.clone()))?;
        registry.register(Box::new(indexer_indexed_blocks.clone()))?;
        registry.register(Box::new(indexer_indexed_transactions.clone()))?;
        registry.register(Box::new(indexer_indexed_accounts.clone()))?;
        registry.register(Box::new(indexer_indexed_notifications.clone()))?;
        registry.register(Box::new(indexer_indexed_notification_accounts.clone()))?;
        registry.register(Box::new(indexer_blocks_behind.clone()))?;
        registry.register(Box::new(indexer_synced.clone()))?;

        let network_label = format!("0x{:08X}", node.chain_spec().network_magic());
        info.with_label_values(&[env!("CARGO_PKG_VERSION"), network_label.as_str()])
            .set(1);

        Ok(Self {
            registry,
            node,
            services,
            started_at: Instant::now(),
            up,
            info,
            uptime_seconds,
            ledger_height,
            connected_peers,
            mempool_transactions,
            mempool_verified_transactions,
            mempool_unverified_transactions,
            header_cache_entries,
            service_enabled,
            indexer_up,
            indexer_indexed_height,
            indexer_indexed_blocks,
            indexer_indexed_transactions,
            indexer_indexed_accounts,
            indexer_indexed_notifications,
            indexer_indexed_notification_accounts,
            indexer_blocks_behind,
            indexer_synced,
            network_label,
        })
    }

    pub(super) fn render(&self) -> anyhow::Result<Vec<u8>> {
        self.refresh();
        let mut metric_families = self.registry.gather();
        metric_families.extend(prometheus::gather());

        let mut buffer = Vec::new();
        TextEncoder::new()
            .encode(&metric_families, &mut buffer)
            .context("encoding Prometheus metrics")?;

        // Append the sync-pipeline metrics (lock-free atomics, not Prometheus
        // collectors) so /metrics exposes per-stage timing + throughput.
        buffer.extend_from_slice(crate::node::sync_metrics::render_prometheus().as_bytes());
        buffer.extend_from_slice(crate::node::tasks::render_prometheus().as_bytes());
        buffer.extend_from_slice(self.render_storage_backend_metrics().as_bytes());

        Ok(buffer)
    }

    fn render_storage_backend_metrics(&self) -> String {
        let storage = self.node.storage();
        if let Some(info) = storage.mdbx_environment_info() {
            return render_mdbx_metrics(info);
        }
        String::new()
    }
}

fn render_mdbx_metrics(info: neo_storage::StorageResult<MdbxEnvironmentInfo>) -> String {
    let info = match info {
        Ok(info) => info,
        Err(err) => {
            warn!(target: "neo::telemetry", error = %err, "failed to read MDBX environment info");
            return String::new();
        }
    };

    format!(
        "# HELP neo_storage_mdbx_map_size_bytes Current MDBX memory-map size in bytes\n\
         # TYPE neo_storage_mdbx_map_size_bytes gauge\n\
         neo_storage_mdbx_map_size_bytes {}\n\
         # HELP neo_storage_mdbx_last_page_number Last used MDBX page number\n\
         # TYPE neo_storage_mdbx_last_page_number gauge\n\
         neo_storage_mdbx_last_page_number {}\n\
         # HELP neo_storage_mdbx_last_transaction_id Last committed MDBX transaction id\n\
         # TYPE neo_storage_mdbx_last_transaction_id gauge\n\
         neo_storage_mdbx_last_transaction_id {}\n\
         # HELP neo_storage_mdbx_max_readers Configured MDBX reader slot capacity reported by the environment\n\
         # TYPE neo_storage_mdbx_max_readers gauge\n\
         neo_storage_mdbx_max_readers {}\n\
         # HELP neo_storage_mdbx_reader_slots_used MDBX reader slots currently used\n\
         # TYPE neo_storage_mdbx_reader_slots_used gauge\n\
         neo_storage_mdbx_reader_slots_used {}\n",
        info.map_size, info.last_pgno, info.last_txnid, info.max_readers, info.num_readers,
    )
}

impl<P, S> MetricsExporter<P, S>
where
    P: NativeContractProvider + 'static,
    S: TransactionalStore + 'static,
{
    pub(super) fn readiness_response(&self) -> Response<Body> {
        let ledger_height = self.ledger_height();
        let remote_ledger = self.remote_ledger_status();
        let indexer = indexer_readiness(ledger_height, self.indexer_status());
        let ready = ledger_height.is_some() && indexer.ready;
        let local_info = self.node.network().local_node_info();
        let mempool = self.node.mempool();

        readiness_response(ReadinessSnapshot {
            ready,
            network_label: self.network_label.clone(),
            ledger_source: if remote_ledger.is_some() {
                "remote_rpc"
            } else {
                "local"
            },
            remote_ledger_rpc: remote_ledger.as_ref().map(|status| status.endpoint.clone()),
            remote_ledger_error: remote_ledger.and_then(|status| status.tip_error.clone()),
            ledger_height,
            connected_peers: local_info.connected_peers_count(),
            mempool_transactions: mempool.total_count(),
            header_cache_entries: self.node.header_cache().count(),
            state_service_enabled: self.state_service_enabled(),
            indexer,
            application_logs_enabled: self.application_logs_enabled(),
            tokens_tracker_enabled: self.tokens_tracker_enabled(),
        })
    }

    fn refresh(&self) {
        self.up.set(1);
        self.info
            .with_label_values(&[env!("CARGO_PKG_VERSION"), self.network_label.as_str()])
            .set(1);
        self.uptime_seconds
            .set(self.started_at.elapsed().as_secs_f64());

        let ledger_height = self.ledger_height();
        self.ledger_height
            .set(ledger_height.map(i64::from).unwrap_or_default());

        let local_info = self.node.network().local_node_info();
        self.connected_peers
            .set(usize_to_i64(local_info.connected_peers_count()));

        let mempool = self.node.mempool();
        self.mempool_transactions
            .set(usize_to_i64(mempool.total_count()));
        self.mempool_verified_transactions
            .set(usize_to_i64(mempool.verified_count()));
        self.mempool_unverified_transactions
            .set(usize_to_i64(mempool.unverified_count()));
        self.header_cache_entries
            .set(usize_to_i64(self.node.header_cache().count()));

        self.refresh_service_metrics(ledger_height);
    }

    fn ledger_height(&self) -> Option<u32> {
        observability_ledger_height(&self.node, &self.services)
    }

    fn remote_ledger_status(&self) -> Option<Arc<RemoteLedgerStatus>> {
        self.services.remote_ledger()
    }

    fn refresh_service_metrics(&self, ledger_height: Option<u32>) {
        self.service_enabled
            .with_label_values(&["state_service"])
            .set(bool_to_i64(self.state_service_enabled()));
        self.service_enabled
            .with_label_values(&["indexer"])
            .set(bool_to_i64(self.indexer_enabled()));
        self.service_enabled
            .with_label_values(&["application_logs"])
            .set(bool_to_i64(self.application_logs_enabled()));
        self.service_enabled
            .with_label_values(&["tokens_tracker"])
            .set(bool_to_i64(self.tokens_tracker_enabled()));

        match self.indexer_status() {
            Some(Ok(status)) => {
                self.indexer_up.set(1);
                self.indexer_indexed_height
                    .set(status.indexed_height.map(i64::from).unwrap_or(-1));
                self.indexer_indexed_blocks
                    .set(usize_to_i64(status.indexed_blocks));
                self.indexer_indexed_transactions
                    .set(usize_to_i64(status.indexed_transactions));
                self.indexer_indexed_accounts
                    .set(usize_to_i64(status.indexed_accounts));
                self.indexer_indexed_notifications
                    .set(usize_to_i64(status.indexed_notifications));
                self.indexer_indexed_notification_accounts
                    .set(usize_to_i64(status.indexed_notification_accounts));
                self.indexer_blocks_behind.set(
                    status
                        .blocks_behind(ledger_height)
                        .map(i64::from)
                        .unwrap_or(-1),
                );
                self.indexer_synced
                    .set(bool_to_i64(status.is_synced_with(ledger_height)));
            }
            Some(Err(_)) | None => {
                self.indexer_up.set(0);
                self.indexer_indexed_height.set(-1);
                self.indexer_indexed_blocks.set(0);
                self.indexer_indexed_transactions.set(0);
                self.indexer_indexed_accounts.set(0);
                self.indexer_indexed_notifications.set(0);
                self.indexer_indexed_notification_accounts.set(0);
                self.indexer_blocks_behind.set(-1);
                self.indexer_synced.set(0);
            }
        }
    }

    fn indexer_status(&self) -> Option<Result<neo_indexer::IndexerStatus, String>> {
        self.services
            .indexer()
            .map(|indexer| indexer.try_status().map_err(|error| error.to_string()))
    }

    fn state_service_enabled(&self) -> bool {
        self.services.state_store().is_some()
    }

    fn indexer_enabled(&self) -> bool {
        self.services.indexer().is_some()
    }

    fn application_logs_enabled(&self) -> bool {
        self.services.application_logs().is_some()
    }

    fn tokens_tracker_enabled(&self) -> bool {
        self.services.tokens_tracker().is_some()
    }
}

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn bool_to_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}
