use super::TaskManager;
use crate::ledger::{PersistCompleted, RelayResult};
use crate::neo_system::NeoSystemContext;
use crate::runtime::ActorContext;
use crate::UInt256;
use std::sync::Arc;
use tracing::trace;

impl TaskManager {
    pub(super) fn attach_system(&mut self, context: Arc<NeoSystemContext>, ctx: &ActorContext) {
        trace!(target: "neo", "task manager attached to system context");
        let capacity = context.memory_pool().lock().capacity.max(100);
        self.known_hashes.set_capacity(capacity);
        let stream = context.event_stream();
        stream.subscribe::<PersistCompleted>(ctx.self_ref());
        stream.subscribe::<RelayResult>(ctx.self_ref());
        self.event_stream = Some(stream);
        self.system = Some(context);
    }

    pub(super) fn increment_inv_task(&mut self, hash: UInt256) -> bool {
        self.global_inv_tasks.try_increment(hash)
    }

    pub(super) fn decrement_inv_task(&mut self, hash: &UInt256) {
        self.global_inv_tasks.decrement(hash);
    }

    pub(super) fn increment_index_task(&mut self, index: u32) -> bool {
        self.global_index_tasks.try_increment(index)
    }

    pub(super) fn decrement_index_task(&mut self, index: u32) {
        self.global_index_tasks.decrement(&index);
    }

    pub(super) fn forget_hash(&mut self, hash: &UInt256) {
        self.known_hashes.remove(hash);
    }

    pub(super) fn is_known_hash(&self, hash: &UInt256) -> bool {
        self.known_hashes.contains(hash)
    }
}
