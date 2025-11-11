mod tick;
mod update;

use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use neo_consensus::{DbftEngine, SnapshotKey};
use neo_runtime::Runtime;
use tokio::{sync::RwLock, time::Duration};

use crate::NodeStatus;

pub(crate) async fn run_background_tasks(
    state: Arc<RwLock<NodeStatus>>,
    consensus: Arc<RwLock<DbftEngine>>,
    store: crate::SharedStore,
    snapshot_key: SnapshotKey,
    runtime: Arc<RwLock<Runtime>>,
    stale_after_ms: u128,
    runtime_snapshot_path: PathBuf,
) -> Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        let tick = tick::collect_tick(
            &consensus,
            store.clone(),
            snapshot_key,
            &runtime,
            runtime_snapshot_path.clone(),
        )
        .await?;
        update::apply_tick(&state, tick, stale_after_ms).await;
    }
}
