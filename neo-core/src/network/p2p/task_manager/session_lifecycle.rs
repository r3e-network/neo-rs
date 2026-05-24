use super::{SessionEntry, TaskManager};
use crate::akka::{ActorContext, ActorRef};
use crate::network::p2p::payloads::VersionPayload;
use crate::network::p2p::task_session::TaskSession;
use tracing::trace;

impl TaskManager {
    pub(super) fn register_session(
        &mut self,
        actor: ActorRef,
        version: VersionPayload,
        ctx: &mut ActorContext,
    ) {
        let path = actor.path().to_string();
        if self.sessions.contains_key(&path) {
            trace!(target: "neo", actor = %path, "task session already registered");
            return;
        }

        if let Err(err) = ctx.watch(&actor) {
            trace!(
                target: "neo",
                actor = %path,
                error = %err,
                "ignoring peer session that could not be watched"
            );
            return;
        }

        let session = TaskSession::new(&version);
        self.sessions.insert(
            path.clone(),
            SessionEntry {
                actor: actor.clone(),
                session,
            },
        );
        trace!(target: "neo", actor = %path, "task session registered successfully");
        self.request_tasks_for_path(&path);
    }

    pub(super) fn update_session(&mut self, actor: &ActorRef, last_block_index: u32) {
        let path = actor.path().to_string();
        if let Some(entry) = self.sessions.get_mut(&path) {
            entry.session.update_last_block_index(last_block_index);
        }
        self.request_tasks_for_path(&path);
    }

    pub(super) fn remove_session_by_ref(&mut self, actor: &ActorRef) {
        let path = actor.path().to_string();
        if let Some(entry) = self.sessions.remove(&path) {
            for hash in entry.session.inv_tasks.keys() {
                self.decrement_inv_task(hash);
            }
            for index in entry.session.index_tasks.keys() {
                self.decrement_index_task(*index);
            }
        }
        self.request_tasks_all();
    }
}
