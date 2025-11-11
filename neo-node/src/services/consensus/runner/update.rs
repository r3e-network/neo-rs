use std::sync::Arc;

use tokio::sync::RwLock;

use crate::NodeStatus;

use super::tick::TickData;

pub(crate) async fn apply_tick(
    state: &Arc<RwLock<NodeStatus>>,
    tick: TickData,
    stale_after_ms: u128,
) {
    let mut guard = state.write().await;
    guard.apply_consensus(
        tick.height,
        tick.view,
        tick.participation,
        tick.tallies,
        tick.quorum,
        tick.primary,
        tick.validators,
        tick.missing,
        tick.expected,
        tick.change_view_reason_counts,
        tick.change_view_reasons,
        tick.change_view_total,
        tick.stages,
        stale_after_ms,
    );
    guard.apply_runtime(&tick.stats);
}
