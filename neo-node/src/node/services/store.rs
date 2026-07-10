//! Service-store opening and fast-sync backend mode.

use std::path::Path;
use std::sync::Arc;

use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::{Store, StoreFactory};
use tracing::{debug, info};

use crate::node::config::{StorageSection, network_scoped_path};

pub(in crate::node) type ServiceStore = Arc<RuntimeStore>;

pub(in crate::node) fn open_service_store_with_storage_config(
    service_name: &'static str,
    storage_provider: &str,
    storage: &StorageSection,
    path: &Path,
    network: u32,
    fast_sync: bool,
) -> anyhow::Result<ServiceStore> {
    let path = network_scoped_path(path, network);
    info!(target: "neo", service = service_name, backend = storage_provider, path = %path.display(), "opening service store");
    let cfg = storage.storage_config_for_path(path);
    let store =
        StoreFactory::get_runtime_store_with_config(storage_provider, cfg).map_err(|err| {
            anyhow::anyhow!("failed to open {service_name} {storage_provider} store: {err}")
        })?;
    if fast_sync && store.supports_fast_sync_mode() {
        store.enable_fast_sync_mode();
        debug!(target: "neo", service = service_name, "enabled service store fast-sync mode");
    }
    Ok(store)
}
