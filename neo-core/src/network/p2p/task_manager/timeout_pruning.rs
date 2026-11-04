use super::TaskManager;
use super::peer_commands::send_get_blocks_by_index;

impl TaskManager {
    pub(super) fn prune_timeouts(&mut self) {
        let timeout = self.task_timeout;
        let recover_frontier =
            self.last_persisted_at.elapsed() >= super::PERSISTENCE_FRONTIER_TIMEOUT;
        let frontier = self.last_seen_persisted_index.saturating_add(1);

        tracing::info!(
            target: "neo",
            persisted_height = self.last_seen_persisted_index,
            global_index_tasks = self.global_index_tasks.len(),
            global_inv_tasks = self.global_inv_tasks.len(),
            sessions = self.sessions.len(),
            "task_manager timer tick"
        );

        let session_paths: Vec<String> = self.sessions.keys().cloned().collect();
        let mut request_cooldown = Vec::new();
        for path in &session_paths {
            self.with_session_mut(path, |entry, this| {
                let expired = entry.session.prune_timed_out_inv_tasks(timeout);
                for hash in expired {
                    this.decrement_inv_task(&hash);
                }

                let expired_indexes = entry.session.prune_timed_out_index_tasks(timeout);
                let had_expired_indexes = !expired_indexes.is_empty();
                for index in expired_indexes {
                    this.decrement_index_task(index);
                }
                if had_expired_indexes {
                    // Do not let a peer whose batch just expired immediately
                    // reclaim the whole window before healthy peers can take it.
                    request_cooldown.push(path.clone());
                }

                let persisted = this.last_seen_persisted_index;
                entry.session.retain_received_blocks_after(persisted);
                if recover_frontier && this.global_index_tasks.has_capacity_for(&frontier) {
                    // Only peers that can actually receive the direct retry may
                    // evict cached blocks. Previously every session cleared its
                    // frontier and later cache even after the global redundancy
                    // limit was reached, amplifying both traffic and gaps.
                    entry.session.remove_received_block(frontier);
                    if entry.session.complete_index_task(frontier) {
                        this.decrement_index_task(frontier);
                    }
                    // Bypass available-inventory scheduling, which can otherwise
                    // keep returning before the exact frontier is requested.
                    entry.session.reserve_received_block_slot_for(frontier);
                    if this.increment_index_task(frontier) {
                        entry.session.register_index_task(frontier);
                        if let Err(error) = send_get_blocks_by_index(&entry.actor, frontier, 1) {
                            tracing::warn!(
                                target: "neo",
                                actor = %entry.actor.path(),
                                retry_height = frontier,
                                %error,
                                "failed to directly retry persistence frontier"
                            );
                            this.decrement_index_task(frontier);
                            entry.session.complete_index_task(frontier);
                        }
                    }
                }
            });
        }
        if recover_frontier {
            tracing::warn!(
                target: "neo",
                persisted_height = self.last_seen_persisted_index,
                retry_height = frontier,
                "recovering stalled persistence frontier"
            );
            self.last_persisted_at = std::time::Instant::now();
        }
        for path in session_paths {
            if !request_cooldown.contains(&path) {
                self.request_tasks_for_path(&path);
            }
        }
    }
}
