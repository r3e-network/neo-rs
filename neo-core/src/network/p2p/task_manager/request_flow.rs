use super::peer_commands::{send_get_blocks_by_index, send_get_data_groups, send_get_headers};
use super::scheduling::{
    block_index_window_limit, effective_header_height, plan_available_inventory_tasks,
    plan_block_index_request, plan_header_request, AvailableInventoryPlan,
};
use super::{request_mempool_once, SessionEntry, TaskManager, HEADER_TASK_HASH};
use crate::akka::ActorRef;
use crate::network::p2p::payloads::{
    inv_payload::{InvPayload, MAX_HASHES_COUNT},
    InventoryType,
};
use crate::smart_contract::native::LedgerContract;
use std::sync::Arc;
use tracing::{trace, warn};

impl TaskManager {
    pub(super) fn request_tasks_for_path(&mut self, path: &str) {
        self.with_session_mut(path, |entry, this| {
            this.request_tasks_entry(entry);
        });
    }

    pub(super) fn request_tasks_all(&mut self) {
        let session_paths: Vec<String> = self.sessions.keys().cloned().collect();
        for path in session_paths {
            self.with_session_mut(&path, |entry, this| {
                this.request_tasks_entry(entry);
            });
        }
    }

    pub(super) fn request_tasks_entry(&mut self, entry: &mut SessionEntry) {
        if entry.session.has_too_many_tasks() {
            return;
        }

        let system = match &self.system {
            Some(context) => Arc::clone(context),
            None => return,
        };

        let actor = entry.actor.clone();
        let session = &mut entry.session;

        let store_cache = system.store_cache();
        let ledger_contract = LedgerContract::new();

        let current_height = ledger_contract
            .current_index(&store_cache)
            .unwrap_or(0)
            .max(self.last_seen_persisted_index);

        trace!(
            target: "neo",
            actor = %actor.path(),
            current_height = current_height,
            peer_height = session.last_block_index,
            available_tasks = session.available_tasks.len(),
            inv_tasks = session.inv_tasks.len(),
            index_tasks = session.index_tasks.len(),
            "requesting tasks from peer"
        );

        if !session.available_tasks.is_empty() {
            let available_plan = plan_available_inventory_tasks(
                session.available_tasks.iter().copied(),
                |hash| {
                    self.known_hashes.contains(hash)
                        || ledger_contract.contains_block(&store_cache, hash)
                },
                &self.global_inv_tasks,
            );
            let AvailableInventoryPlan {
                stale,
                scheduled: candidates,
            } = available_plan;
            for hash in stale {
                session.available_tasks.remove(&hash);
            }

            let mut scheduled = Vec::with_capacity(candidates.len());
            let mut to_remove = Vec::with_capacity(candidates.len());
            for hash in candidates {
                if self.increment_inv_task(hash) {
                    session.register_inv_task(hash);
                    scheduled.push(hash);
                    to_remove.push(hash);
                }
            }
            for hash in to_remove {
                session.available_tasks.remove(&hash);
            }

            if !scheduled.is_empty() {
                send_get_data_groups(
                    &actor,
                    InventoryType::Block,
                    scheduled,
                    "failed to request available tasks from peer",
                );
                return;
            }
        }

        let current_height = ledger_contract
            .current_index(&store_cache)
            .unwrap_or(0)
            .max(self.last_seen_persisted_index);

        let ledger = system.ledger();
        let header_cache = system.header_cache();
        let header_height = effective_header_height(
            header_cache.last().map(|header| header.index()),
            ledger.highest_header_index(),
        );
        let header_request_start = header_height.saturating_add(1);
        let header_task_count = self
            .global_inv_tasks
            .get(&HEADER_TASK_HASH)
            .copied()
            .unwrap_or(0);
        let header_request = plan_header_request(
            header_height,
            session.last_block_index,
            header_task_count,
            session.should_request_headers(header_request_start, self.task_timeout),
        );
        if let Some(header_request) =
            header_request.filter(|_| self.increment_inv_task(HEADER_TASK_HASH))
        {
            session.register_inv_task(HEADER_TASK_HASH);
            session.record_header_request(header_request.start_index);
            if let Err(error) = send_get_headers(&actor, header_request.start_index) {
                warn!(
                    target: "neo",
                    actor = %actor.path(),
                    %error,
                    "failed to request headers from peer"
                );
                self.decrement_inv_task(&HEADER_TASK_HASH);
                session.complete_inv_task(&HEADER_TASK_HASH);
            }
            // Do NOT return here: fast sync pipelines block requests alongside
            // header fetches so block download is not starved by long header sync.
        }

        if current_height < session.last_block_index {
            let plan = plan_block_index_request(
                current_height,
                session.last_block_index,
                &self.global_index_tasks,
            );

            if self.global_index_tasks.is_empty() {
                tracing::info!(
                    target: "neo",
                    actor = %actor.path(),
                    start_height = plan
                        .map(|plan| plan.start_height)
                        .unwrap_or_else(|| current_height.saturating_add(1)),
                    limit_height = block_index_window_limit(current_height),
                    peer_height = session.last_block_index,
                    current_height = current_height,
                    "block index request - pipeline empty"
                );
            }

            if let Some(plan) = plan {
                let mut granted = 0u32;
                for offset in 0..plan.count {
                    let index = plan.start_height + offset;
                    if self.increment_index_task(index) {
                        session.register_index_task(index);
                        granted += 1;
                    }
                }

                if granted > 0 {
                    if let Err(error) =
                        send_get_blocks_by_index(&actor, plan.start_height, granted as i16)
                    {
                        warn!(
                            target: "neo",
                            actor = %actor.path(),
                            %error,
                            "failed to request blocks by index from peer"
                        );
                        for offset in 0..granted {
                            let index = plan.start_height + offset;
                            self.decrement_index_task(index);
                            session.complete_index_task(index);
                        }
                    }
                    return;
                }
            }
        }

        request_mempool_once(&actor, session);
    }

    pub(super) fn with_session_mut<F>(&mut self, path: &str, mut f: F)
    where
        F: FnMut(&mut SessionEntry, &mut TaskManager),
    {
        if let Some(mut entry) = self.sessions.remove(path) {
            f(&mut entry, self);
            self.sessions.insert(path.to_string(), entry);
        }
    }

    pub(super) fn on_headers(&mut self, actor: &ActorRef) {
        let path = actor.path().to_string();
        if !self.sessions.contains_key(&path) {
            trace!(target: "neo", actor = %path, "ignoring headers from unknown session");
            return;
        }

        self.with_session_mut(&path, |entry, this| {
            if entry.session.complete_inv_task(&HEADER_TASK_HASH) {
                this.decrement_inv_task(&HEADER_TASK_HASH);
            }
            this.request_tasks_entry(entry);
        });
    }

    pub(super) fn on_new_tasks(&mut self, actor: &ActorRef, payload: InvPayload) {
        let path = actor.path().to_string();
        if payload.is_empty() {
            return;
        }

        if !self.sessions.contains_key(&path) {
            trace!(target: "neo", actor = %path, "ignoring NewTasks from unknown session");
            return;
        }

        let system = match &self.system {
            Some(context) => Arc::clone(context),
            None => return,
        };

        let store_cache = system.store_cache();
        let ledger_contract = LedgerContract::new();
        let current_height = ledger_contract
            .current_index(&store_cache)
            .unwrap_or(0)
            .max(self.last_seen_persisted_index);
        let ledger = system.ledger();
        let header_cache = system.header_cache();
        let header_height = effective_header_height(
            header_cache.last().map(|header| header.index()),
            ledger.highest_header_index(),
        );
        drop(store_cache);

        let inventory_type = payload.inventory_type;
        let hashes = payload.hashes.clone();
        let actor_ref = actor.clone();
        self.with_session_mut(&path, move |entry, this| {
            if current_height < header_height
                && (inventory_type == InventoryType::Transaction
                    || (inventory_type == InventoryType::Block
                        && current_height
                            < entry
                                .session
                                .last_block_index
                                .saturating_sub(MAX_HASHES_COUNT as u32)))
            {
                this.request_tasks_entry(entry);
                return;
            }

            let mut pending = Vec::with_capacity(hashes.len());
            for hash in hashes.iter().copied() {
                if this.is_known_hash(&hash) {
                    continue;
                }

                if inventory_type == InventoryType::Block
                    && this.global_inv_tasks.contains_key(&hash)
                {
                    entry.session.available_tasks.insert(hash);
                }

                if this.global_inv_tasks.contains_key(&hash) {
                    continue;
                }

                pending.push(hash);
            }

            if pending.is_empty() {
                this.request_tasks_entry(entry);
                return;
            }

            let mut scheduled = Vec::with_capacity(pending.len());
            for hash in pending.into_iter() {
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
                    "failed to request inventory data from peer",
                );
            } else {
                this.request_tasks_entry(entry);
            }
        });

        self.request_tasks_for_path(&path);
    }
}
