//! # neo-node::node::services
//!
//! Auxiliary service startup and handles used by the daemon.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `read_side`: indexer, application logs, and token-tracker construction.
//! - `handles`: Typed optional-service ownership passed to daemon consumers.
//! - `state`: StateService MPT store and commit-handler construction.
//! - `store`: service-store opening and fast-sync backend mode.

use std::sync::Arc;

use neo_storage::persistence::providers::RuntimeStore;
use tracing::info;

use super::config::{NodeConfig, service_store_provider};

mod handles;
mod read_side;
mod state;
mod store;

pub(super) use handles::NodeServiceHandles;
use read_side::{ReadSideServices, TokensTrackerRuntime};
use state::StateServiceRuntime;
use store::ServiceStore;
#[cfg(test)]
pub(in crate::node) use store::open_service_store_with_storage_config;

pub(super) struct OperationalServices {
    pub(super) state_store: Option<Arc<neo_state_service::StateStore<RuntimeStore>>>,
    pub(super) state_service:
        Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers<RuntimeStore>>>,
    pub(super) indexer_service: Option<Arc<neo_indexer::IndexerService>>,
    pub(super) application_logs_service:
        Option<Arc<neo_rpc::application_logs::ApplicationLogsService<RuntimeStore>>>,
    pub(super) tokens_tracker_service:
        Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTrackerService<RuntimeStore>>>,
    pub(super) tokens_tracker_runtime: Option<TokensTrackerRuntime>,
    pub(super) durable_stores: Vec<ServiceStore>,
}

pub(super) fn build_operational_services(
    config: &NodeConfig,
    network: u32,
    enable_local_replay_services: bool,
    service_fast_sync: bool,
    canonical_store: &Arc<RuntimeStore>,
) -> anyhow::Result<OperationalServices> {
    if !enable_local_replay_services {
        info!(
            target: "neo::remote_ledger",
            "local replay-derived services disabled for remote-ledger mode"
        );
        return Ok(OperationalServices {
            state_store: None,
            state_service: None,
            indexer_service: None,
            application_logs_service: None,
            tokens_tracker_service: None,
            tokens_tracker_runtime: None,
            durable_stores: Vec::new(),
        });
    }

    let storage_provider = service_store_provider(config)?;
    let state_runtime = state::build_state_service_runtime(
        config,
        network,
        &storage_provider,
        service_fast_sync,
        canonical_store,
    )?;
    let read_side_services =
        read_side::build_read_side_services(config, network, &storage_provider)?;
    let StateServiceRuntime {
        state_store,
        state_service,
        durable_store: state_service_store,
    } = state_runtime;
    let ReadSideServices {
        indexer_service,
        application_logs_service,
        tokens_tracker_service,
        tokens_tracker_runtime,
    } = read_side_services;
    let durable_stores = state_service_store.into_iter().collect();

    Ok(OperationalServices {
        state_store,
        state_service,
        indexer_service,
        application_logs_service,
        tokens_tracker_service,
        tokens_tracker_runtime,
        durable_stores,
    })
}
