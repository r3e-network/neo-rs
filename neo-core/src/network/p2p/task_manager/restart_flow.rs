use super::peer_commands::send_get_data_groups;
use super::TaskManager;
use crate::network::p2p::payloads::{inv_payload::InvPayload, InventoryType};
use crate::runtime::ActorRef;
use crate::UInt256;
use tracing::trace;

impl TaskManager {
    pub(super) fn restart_tasks_for_session(
        &mut self,
        path: &str,
        inventory_type: InventoryType,
        hashes: &[UInt256],
    ) {
        self.with_session_mut(path, move |entry, this| {
            let actor_ref = entry.actor.clone();
            let mut scheduled = Vec::with_capacity(hashes.len());
            for &hash in hashes {
                if this.increment_inv_task(hash) {
                    entry.session.register_inv_task(hash);
                    scheduled.push(hash);
                }
            }

            if !scheduled.is_empty() {
                send_get_data_groups(
                    &actor_ref,
                    inventory_type,
                    scheduled,
                    "failed to restart inventory fetch from peer",
                );
            }
        });
    }

    pub(super) fn on_restart_tasks(&mut self, actor: Option<ActorRef>, payload: InvPayload) {
        let inventory_type = payload.inventory_type;
        let hashes: Vec<UInt256> = payload.hashes.clone();

        if let Some(actor_ref) = actor {
            let path = actor_ref.path().to_string();
            if self.sessions.contains_key(&path) {
                for hash in hashes.iter() {
                    self.forget_hash(hash);
                    self.decrement_inv_task(hash);
                }
                self.restart_tasks_for_session(&path, inventory_type, &hashes);
                self.request_tasks_for_path(&path);
                return;
            }

            trace!(
                target: "neo",
                actor = %path,
                "broadcasting RestartTasks from unknown session"
            );
        }

        for hash in hashes.iter() {
            self.forget_hash(hash);
            while self.global_inv_tasks.contains_key(hash) {
                self.decrement_inv_task(hash);
            }
        }

        let session_paths: Vec<String> = self.sessions.keys().cloned().collect();
        for path in session_paths {
            self.restart_tasks_for_session(&path, inventory_type, &hashes);
        }
    }
}
