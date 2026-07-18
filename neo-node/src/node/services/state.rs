//! StateService MPT store and commit-handler construction.

use std::sync::Arc;

use anyhow::Context;
use neo_state_packs::authority::AUTHORITATIVE_HIGH_WATER_KEY;
use neo_storage::persistence::TransactionalStore;
use neo_storage::persistence::providers::{MemoryStore, RuntimeStore};
use tracing::info;

use crate::node::config::{NodeConfig, network_scoped_path};
use crate::node::inventory_relay::FAST_SYNC_BURST_CAPACITY;

use super::store::{ServiceStore, open_service_store_with_storage_config};

/// Large ordered MPT batches amortize one MDBX commit across the trusted
/// chain.acc catch-up burst. The queue remains separately bounded, so this
/// only changes how much already-projected work the single ordered writer
/// consumes per durable transaction.
const FAST_SYNC_MPT_APPLY_BATCH_BLOCKS: usize = FAST_SYNC_BURST_CAPACITY;

pub(super) struct StateServiceRuntime {
    pub(super) state_store: Option<Arc<neo_state_service::StateStore<RuntimeStore>>>,
    pub(super) state_service:
        Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers<RuntimeStore>>>,
    pub(super) durable_store: Option<ServiceStore>,
    pub(super) authoritative_pack: Option<Arc<crate::node::state_packs::AuthoritativeNodePack>>,
}

pub(super) fn build_state_service_runtime(
    config: &NodeConfig,
    network: u32,
    storage_provider: &str,
    use_bulk_state_pipeline: bool,
    canonical_store: &Arc<RuntimeStore>,
) -> anyhow::Result<StateServiceRuntime> {
    let use_async_state_pipeline =
        use_bulk_state_pipeline && config.state_service.track_during_catchup;
    let (state_store, durable_store, coordinated, authoritative_pack) =
        build_state_store(config, network, storage_provider, canonical_store)?;
    let state_service = match (state_store.as_ref(), coordinated) {
        (Some(state_store), true) => Some(Arc::new(
            neo_state_service::commit_handlers::StateServiceCommitHandlers::try_new_coordinated(
                Arc::clone(state_store),
            )
            .map_err(anyhow::Error::msg)
            .context("constructing coordinated StateService commit handlers")?,
        )),
        (Some(state_store), false) if use_async_state_pipeline => {
            let handlers =
                neo_state_service::commit_handlers::StateServiceCommitHandlers::try_new_async_with_limits(
                    Arc::clone(state_store),
                    FAST_SYNC_BURST_CAPACITY,
                    FAST_SYNC_MPT_APPLY_BATCH_BLOCKS,
                )
                .context("spawning StateService MPT worker")?;
            Some(Arc::new(handlers))
        }
        (Some(state_store), false) => Some(Arc::new(
            neo_state_service::commit_handlers::StateServiceCommitHandlers::new(Arc::clone(
                state_store,
            )),
        )),
        (None, _) => None,
    };

    Ok(StateServiceRuntime {
        state_store,
        state_service,
        durable_store,
        authoritative_pack,
    })
}

fn build_state_store(
    config: &NodeConfig,
    network: u32,
    storage_provider: &str,
    canonical_store: &Arc<RuntimeStore>,
) -> anyhow::Result<(
    Option<Arc<neo_state_service::StateStore<RuntimeStore>>>,
    Option<ServiceStore>,
    bool,
    Option<Arc<crate::node::state_packs::AuthoritativeNodePack>>,
)> {
    if !config.state_service.enabled {
        return Ok((None, None, false, None));
    }

    let mut durable_store = None;
    let mut authoritative_pack = None;
    let coordinated = config.state_service.coordinated
        && storage_provider.eq_ignore_ascii_case("mdbx")
        && canonical_store.as_mdbx().is_some();
    let state_store = if coordinated {
        let backing = Arc::new(
            canonical_store
                .open_coordinated_namespace(neo_state_service::MDBX_STATE_SERVICE_NAMESPACE)
                .context("opening StateService MDBX namespace")?,
        );
        durable_store = Some(Arc::clone(&backing));
        if config.storage.state_packs.enabled {
            let path = network_scoped_path(
                config
                    .storage
                    .state_packs
                    .path
                    .as_deref()
                    .context("validated state-pack path is absent")?,
                network,
            );
            info!(
                target: "neo::state_packs",
                random_point_mmap = config.storage.state_packs.random_point_mmap,
                "opening authoritative node-pack read path"
            );
            let authority = if config.storage.state_packs.random_point_mmap {
                crate::node::state_packs::AuthoritativeNodePack::open_with_random_point_mmap(
                    &path,
                    config.storage.state_packs.max_index_memory_bytes(),
                    network,
                    backing.as_ref(),
                )
            } else {
                crate::node::state_packs::AuthoritativeNodePack::open(
                    &path,
                    config.storage.state_packs.max_index_memory_bytes(),
                    network,
                    backing.as_ref(),
                )
            }
            .with_context(|| format!("opening authoritative node packs at {}", path.display()))?;
            let factory: Arc<dyn neo_state_service::MptNodeSnapshotFactory> = authority.clone();
            let state_store =
                neo_state_service::StateStore::with_mpt_store_and_node_snapshot_options(
                    config.state_service.full_state,
                    config.state_service.defer_full_state_finalization,
                    backing,
                    factory,
                )
                .context("opening coordinated split-store StateService MPT")?;
            authoritative_pack = Some(authority);
            Arc::new(state_store)
        } else {
            if backing
                .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
                .context("checking authoritative state-pack marker")?
                .is_some()
            {
                return Err(anyhow::anyhow!(
                    "authoritative state-pack marker exists; refusing to fall back to the stale MDBX MPT node namespace"
                ));
            }
            Arc::new(
                neo_state_service::StateStore::with_mpt_store_options(
                    config.state_service.full_state,
                    config.state_service.defer_full_state_finalization,
                    backing,
                )
                .context("opening coordinated StateService MPT store")?,
            )
        }
    } else if let Some(path) = &config.state_service.path {
        let backing = open_service_store_with_storage_config(
            "StateService",
            storage_provider,
            &config.storage,
            path,
            network,
        )?;
        durable_store = Some(Arc::clone(&backing));
        Arc::new(
            neo_state_service::StateStore::with_mpt_store_options(
                config.state_service.full_state,
                config.state_service.defer_full_state_finalization,
                backing,
            )
            .with_context(|| {
                format!(
                    "opening StateService MPT store at {}",
                    network_scoped_path(path, network).display()
                )
            })?,
        )
    } else if !config.state_service.coordinated && storage_provider.eq_ignore_ascii_case("mdbx") {
        return Err(anyhow::anyhow!(
            "[state_service].path is required when coordinated=false with MDBX storage"
        ));
    } else {
        let backing: ServiceStore = Arc::new(RuntimeStore::Memory(MemoryStore::new()));
        Arc::new(
            neo_state_service::StateStore::with_mpt_store_options(
                config.state_service.full_state,
                config.state_service.defer_full_state_finalization,
                backing,
            )
            .context("opening in-memory StateService MPT store")?,
        )
    };
    info!(
        target: "neo",
        full_state = config.state_service.full_state,
        defer_full_state_finalization = config.state_service.defer_full_state_finalization,
        coordinated,
        "state service MPT store enabled"
    );
    Ok((
        Some(state_store),
        durable_store,
        coordinated,
        authoritative_pack,
    ))
}
