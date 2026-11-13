use super::block_validation::{
    IncomingBlockDisconnect, IncomingBlockOutcome, PersistedBlockMatch, match_persisted_block,
    persisted_block_hash, validate_incoming_block,
};
use super::peer_commands::disconnect as disconnect_peer;
use super::{HEADER_TASK_HASH, TaskManager};
use crate::network::p2p::payloads::Block;
use neo_actors::ActorRef;
use neo_primitives::{RelayError, UInt256};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{trace, warn};

fn note_received_block_drop(
    actor: &ActorRef,
    block_index: u32,
    retained_blocks: usize,
    retained_bytes: usize,
) {
    static DROPPED: AtomicU64 = AtomicU64::new(0);
    let dropped = DROPPED.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    if dropped & 1023 != 1 {
        return;
    }
    warn!(
        target: "neo",
        actor = %actor.path(),
        block_index,
        retained_blocks,
        retained_bytes,
        dropped,
        "dropping blocks: per-peer received-block budget exceeded"
    );
}

impl TaskManager {
    fn remove_available_task_from_all(&mut self, hash: &UInt256) {
        for entry in self.sessions.values_mut() {
            entry.session.available_tasks.remove(hash);
        }
    }

    fn record_known_hash(&mut self, hash: UInt256) {
        self.known_hashes.try_add(hash);
    }

    pub(super) fn complete_inventory(
        &mut self,
        actor: &ActorRef,
        hash: UInt256,
        block: Option<Block>,
        block_index: Option<u32>,
    ) {
        if hash != HEADER_TASK_HASH {
            self.record_known_hash(hash);
        }
        self.remove_available_task_from_all(&hash);

        trace!(
            target: "neo",
            actor = %actor.path(),
            hash = %hash,
            block_index = ?block_index,
            has_block = block.is_some(),
            "inventory completed"
        );

        let path = actor.path().to_string();
        let Some(mut entry) = self.sessions.remove(&path) else {
            trace!(target: "neo", actor = %path, "inventory completion for unknown session");
            return;
        };

        let actor_ref = entry.actor.clone();

        // Capture whether this block was actually requested before completing
        // the task entries (which removes them from the session).
        let block_was_requested = block.as_ref().is_some_and(|block_ref| {
            let index = block_ref.index();
            entry.session.index_tasks.contains_key(&index)
                || entry.session.inv_tasks.contains_key(&hash)
                || entry.session.received_block.contains_key(&index)
        });

        if entry.session.complete_inv_task(&hash) {
            self.decrement_inv_task(&hash);
        }

        let mut index_to_release = block_index;
        if let Some(ref block_ref) = block {
            index_to_release = Some(block_ref.index());
        }

        if let Some(index) = index_to_release
            && entry.session.complete_index_task(index)
        {
            self.decrement_index_task(index);
        }

        let mut should_request = true;
        let mut disconnect_reason = None;

        if let Some(block_payload) = block {
            // Keep the fast-sync request pipeline full after each delivered block;
            // persistence may lag behind block download by many blocks.
            should_request = true;
            let index = block_payload.index();
            let requested = block_was_requested
                || entry.session.index_tasks.contains_key(&index)
                || entry.session.inv_tasks.contains_key(&hash)
                || entry.session.received_block.contains_key(&index);
            if !requested {
                // Never retain unsolicited blocks: they are the cheapest way for
                // a peer to pin memory before validation.
                trace!(
                    target: "neo",
                    actor = %actor_ref.path(),
                    block_index = index,
                    hash = %hash,
                    "dropping unsolicited block payload"
                );
            } else {
                match validate_incoming_block(
                    block_payload,
                    entry.session.received_block.get(&index),
                ) {
                    IncomingBlockOutcome::Store { index, block } => {
                        if !entry.session.store_received_block(index, block) {
                            // The index task is already complete. Requesting the
                            // same height while the buffer is still full is what
                            // turned one overflow into millions of repeat drops.
                            should_request = false;
                            note_received_block_drop(
                                &actor_ref,
                                index,
                                entry.session.received_block.len(),
                                entry.session.received_block_bytes(),
                            );
                        }
                    }
                    IncomingBlockOutcome::KeepExisting => {}
                    IncomingBlockOutcome::Disconnect {
                        index,
                        reason,
                        hash_error,
                        remove_cached,
                    } => {
                        if let Some(error) = hash_error {
                            match reason {
                                IncomingBlockDisconnect::InvalidIncomingHash => {
                                    warn!(
                                        target: "neo",
                                        actor = %actor_ref.path(),
                                        block_index = index,
                                        %error,
                                        "disconnecting peer after receiving block with unhashable header"
                                    );
                                }
                                IncomingBlockDisconnect::InvalidCachedHash => {
                                    warn!(
                                        target: "neo",
                                        actor = %actor_ref.path(),
                                        block_index = index,
                                        %error,
                                        "discarding cached received block with unhashable header"
                                    );
                                }
                                IncomingBlockDisconnect::ConflictingBlock => {}
                            }
                        }
                        if remove_cached {
                            entry.session.remove_received_block(index);
                        }
                        disconnect_reason = Some(reason.reason().to_string());
                    }
                }
            }
        }

        if hash == HEADER_TASK_HASH {
            entry.session.available_tasks.remove(&hash);
        }

        self.sessions.insert(path.clone(), entry);

        if let Some(reason) = disconnect_reason {
            if let Err(error) = disconnect_peer(&actor_ref, reason) {
                warn!(
                    target: "neo",
                    actor = %actor_ref.path(),
                    %error,
                    "failed to disconnect peer after conflicting block"
                );
            }
            return;
        }

        if should_request {
            self.request_tasks_for_path(&path);
        }
    }

    pub(super) fn on_persist_completed(&mut self, block: &Block) {
        self.last_seen_persisted_index = block.index();
        self.last_persisted_at = std::time::Instant::now();
        let index = block.index();
        let hash = match persisted_block_hash(block) {
            Ok(hash) => hash,
            Err(error) => {
                warn!(
                    target: "neo",
                    block_index = index,
                    %error,
                    "skipping task-manager persist matching for unhashable persisted block"
                );
                return;
            }
        };

        let session_count = self.sessions.len();
        let mut to_disconnect = Vec::with_capacity(session_count);
        let mut to_request = Vec::with_capacity(session_count);

        for (path, entry) in self.sessions.iter_mut() {
            if let Some(stored) = entry.session.remove_received_block(index) {
                match match_persisted_block(stored, &hash) {
                    PersistedBlockMatch::Matches => {
                        to_request.push(path.clone());
                    }
                    PersistedBlockMatch::Mismatch => {
                        to_disconnect.push(entry.actor.clone());
                    }
                    PersistedBlockMatch::Unhashable(error) => {
                        warn!(
                            target: "neo",
                            actor = %entry.actor.path(),
                            block_index = index,
                            %error,
                            "disconnecting peer after cached block hash failed during persistence match"
                        );
                        to_disconnect.push(entry.actor.clone());
                    }
                }
            }
        }

        for actor in to_disconnect {
            if let Err(error) = disconnect_peer(&actor, "persisted block hash mismatch".to_string())
            {
                warn!(
                    target: "neo",
                    actor = %actor.path(),
                    %error,
                    "failed to disconnect peer after mismatched block persistence"
                );
            }
        }

        for path in to_request {
            self.request_tasks_for_path(&path);
        }

        self.global_index_tasks.retain_tracked(|idx| *idx > index);

        let session_paths: Vec<String> = self.sessions.keys().cloned().collect();
        for path in session_paths {
            self.with_session_mut(&path, |entry, this| {
                entry.session.index_tasks.retain(|&idx, _| idx > index);
                this.request_tasks_entry(entry);
            });
        }
    }

    pub(super) fn on_persist_failed(&mut self, index: u32) {
        // The payload already passed block verification. A local commit failure
        // must not be treated as an invalid peer block, but its completed task
        // and cached payload have to be released so the height can be requested
        // again instead of pinning the sliding window forever.
        let session_paths: Vec<String> = self.sessions.keys().cloned().collect();
        for path in session_paths {
            self.with_session_mut(&path, |entry, this| {
                entry.session.remove_received_block(index);
                if entry.session.complete_index_task(index) {
                    this.decrement_index_task(index);
                }
            });
        }
        self.request_tasks_all();
    }

    pub(super) fn on_relay_result(&mut self, result: &neo_ledger::RelayResult<()>) {
        // Handle relay result from blockchain
        //
        // RelayResult<()> is Result<(), RelayError> indicating whether the
        // inventory (block/transaction) was successfully relayed to the network.
        //
        // Success case (Ok(())): Inventory validated and added, normal flow continues
        //
        // Error cases track why relay failed for monitoring and peer reputation

        if let Err(error) = result {
            match error {
                RelayError::ValidationFailed { message } => {
                    tracing::warn!(
                        target: "neo::p2p",
                        message = %message,
                        "block validation failed from peer"
                    );
                    // Future: Track validation failures per peer for reputation
                }
                RelayError::AlreadyExists { hash } => {
                    // Normal P2P behavior - multiple peers may send same block
                    tracing::trace!(
                        target: "neo::p2p",
                        hash = %hash,
                        "duplicate block received"
                    );
                }
                RelayError::TransactionInvalid { message } => {
                    tracing::warn!(
                        target: "neo::p2p",
                        message = %message,
                        "invalid transaction from peer"
                    );
                    // Future: Penalize peer for invalid transactions
                }
                RelayError::MempoolFull { current, max } => {
                    tracing::debug!(
                        target: "neo::p2p",
                        current = %current,
                        max = %max,
                        "mempool full, relay rejected"
                    );
                    // Future: Temporarily pause requesting transactions
                }
                _ => {
                    // Handle any other relay errors (InvalidHeight, etc.)
                    tracing::debug!(
                        target: "neo::p2p",
                        error = %error,
                        "relay failed"
                    );
                }
            }
        }
    }
}
