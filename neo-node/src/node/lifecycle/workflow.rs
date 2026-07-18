//! Composed node workflow below the application lifecycle.
//!
//! `RunningNode` owns the mutable process resources produced by composition and
//! executes the ordered startup-import, live-service, and shutdown stages. The
//! application layer supplies operator intent and configuration without
//! reaching into service handles, task lists, or durable stores.

use std::sync::Arc;

use neo_storage::persistence::providers::RuntimeStore;
use tokio_util::sync::CancellationToken;
use tracing::info;

use super::cli::{LedgerMode, NodeCli};
use super::config::NodeConfig;
use super::live_services::{start_live_services, start_metrics_endpoint};
use super::observability::ObservabilityRuntime;
use super::services::NodeServiceHandles;
use super::shutdown_flow::run_daemon_shutdown;
use super::startup_import::{StartupImportContext, StartupImportOutcome, run_startup_imports};

type ComposedNode = neo_system::Node<neo_native_contracts::StandardNativeProvider, RuntimeStore>;

/// Fully composed node resources awaiting an operator workflow.
pub(in crate::node) struct RunningNode {
    node: Arc<ComposedNode>,
    network: neo_network::NetworkHandle,
    handles: Vec<tokio::task::JoinHandle<()>>,
    shutdown: CancellationToken,
    services: Arc<NodeServiceHandles<RuntimeStore>>,
    durable_service_stores: Vec<Arc<RuntimeStore>>,
    specialization_control: Option<neo_execution::specialization::SpecializationControl>,
}

impl RunningNode {
    /// Capture one coherent resource graph produced by node composition.
    pub(in crate::node) fn new(
        node: Arc<ComposedNode>,
        network: neo_network::NetworkHandle,
        handles: Vec<tokio::task::JoinHandle<()>>,
        shutdown: CancellationToken,
        services: Arc<NodeServiceHandles<RuntimeStore>>,
        durable_service_stores: Vec<Arc<RuntimeStore>>,
        specialization_control: Option<neo_execution::specialization::SpecializationControl>,
    ) -> Self {
        Self {
            node,
            network,
            handles,
            shutdown,
            services,
            durable_service_stores,
            specialization_control,
        }
    }

    /// Start local metrics, execute startup imports, then start live services.
    pub(in crate::node) async fn run_requested_mode(
        mut self,
        cli: &NodeCli,
        config: &NodeConfig,
        network_magic: u32,
        observability: Option<&ObservabilityRuntime>,
    ) -> anyhow::Result<()> {
        if let Some(control) = self.specialization_control.as_ref() {
            let snapshot = control.snapshot();
            info!(
                target: "neo::specialization",
                strict_replay = snapshot.strict_replay,
                candidates = snapshot.candidates.len(),
                "ordinary-authoritative specialization shadow control active"
            );
        }
        // Archive import is the dominant catch-up workload. Expose its live
        // counters without starting RPC or P2P before local state is ready.
        start_metrics_endpoint(
            &self.node,
            &self.services,
            &mut self.handles,
            &self.shutdown,
            config,
            observability,
        )?;
        let startup_import = run_startup_imports(StartupImportContext {
            cli,
            node: &self.node,
            services: &self.services,
            config,
            network: network_magic,
            durable_service_stores: &self.durable_service_stores,
            handles: &mut self.handles,
            observability,
        })
        .await?;
        if startup_import == StartupImportOutcome::StopHeightReached {
            return Ok(());
        }

        let ledger_mode = LedgerMode::from_cli(cli);
        let _live_service_guards = start_live_services(
            &self.node,
            &self.services,
            &self.network,
            &mut self.handles,
            &self.shutdown,
            config,
            network_magic,
            ledger_mode,
            observability,
        )
        .await?;

        run_daemon_shutdown(
            &self.node,
            &self.services,
            cli.stop_at_height,
            self.shutdown,
            self.handles,
            &self.durable_service_stores,
            observability,
        )
        .await
    }

    #[cfg(test)]
    pub(in crate::node) fn node(&self) -> &Arc<ComposedNode> {
        &self.node
    }

    #[cfg(test)]
    pub(in crate::node) fn network(&self) -> &neo_network::NetworkHandle {
        &self.network
    }

    #[cfg(test)]
    pub(in crate::node) fn services(&self) -> &Arc<NodeServiceHandles<RuntimeStore>> {
        &self.services
    }

    #[cfg(test)]
    pub(in crate::node) fn specialization_control(
        &self,
    ) -> Option<&neo_execution::specialization::SpecializationControl> {
        self.specialization_control.as_ref()
    }

    #[cfg(test)]
    pub(in crate::node) async fn abort_for_test(mut self) {
        let _ = self.network.shutdown().await;
        for handle in self.handles.drain(..) {
            handle.abort();
            let _ = handle.await;
        }
    }
}
