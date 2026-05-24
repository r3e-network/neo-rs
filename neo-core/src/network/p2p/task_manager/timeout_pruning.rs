use super::TaskManager;

impl TaskManager {
    pub(super) fn prune_timeouts(&mut self) {
        let timeout = self.task_timeout;

        tracing::info!(
            target: "neo",
            persisted_height = self.last_seen_persisted_index,
            global_index_tasks = self.global_index_tasks.len(),
            global_inv_tasks = self.global_inv_tasks.len(),
            sessions = self.sessions.len(),
            "task_manager timer tick"
        );

        let session_paths: Vec<String> = self.sessions.keys().cloned().collect();
        for path in session_paths {
            self.with_session_mut(&path, |entry, this| {
                let expired = entry.session.prune_timed_out_inv_tasks(timeout);
                for hash in expired {
                    this.decrement_inv_task(&hash);
                }

                let expired_indexes = entry.session.prune_timed_out_index_tasks(timeout);
                for index in expired_indexes {
                    this.decrement_index_task(index);
                }

                let persisted = this.last_seen_persisted_index;
                entry
                    .session
                    .received_block
                    .retain(|&idx, _| idx > persisted);
            });
        }
        self.request_tasks_all();
    }
}
