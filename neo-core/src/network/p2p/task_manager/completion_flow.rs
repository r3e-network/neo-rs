use super::block_validation::{
    block_matches_hash, match_persisted_block, persisted_block_hash, validate_incoming_block,
    BlockHashMatch, IncomingBlockDisconnect, IncomingBlockOutcome, PersistedBlockMatch,
};
use super::peer_commands::disconnect as disconnect_peer;
use super::{TaskManager, HEADER_TASK_HASH};
use crate::akka::ActorRef;
use crate::ledger::{RelayResult, VerifyResult};
use crate::network::p2p::payloads::{block::Block, InventoryType};
use crate::UInt256;
use tracing::{trace, warn};

impl TaskManager {
    fn remove_available_task_from_all(&mut self, hash: &UInt256) {
        for entry in self.sessions.values_mut() {
            entry.session.available_tasks.remove(hash);
        }
    }

    fn record_known_hash(&mut self, hash: UInt256) {
        self.known_hashes.remember(hash);
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

        if entry.session.complete_inv_task(&hash) {
            self.decrement_inv_task(&hash);
        }

        let mut index_to_release = block_index;
        if let Some(ref block_ref) = block {
            index_to_release = Some(block_ref.index());
        }

        if let Some(index) = index_to_release {
            if entry.session.complete_index_task(index) {
                self.decrement_index_task(index);
            }
        }

        let mut should_request = true;
        let mut disconnect_reason = None;

        if let Some(block_payload) = block {
            // Keep the fast-sync request pipeline full after each delivered block;
            // persistence may lag behind block download by many blocks.
            should_request = true;
            let index = block_payload.index();
            match validate_incoming_block(block_payload, entry.session.received_block.get(&index)) {
                IncomingBlockOutcome::Store { index, block } => {
                    entry.session.store_received_block(index, block);
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
                        entry.session.received_block.remove(&index);
                    }
                    disconnect_reason = Some(reason.reason().to_string());
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
            if let Some(stored) = entry.session.received_block.remove(&index) {
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

        self.global_index_tasks.retain(|&idx, _| idx > index);

        let session_paths: Vec<String> = self.sessions.keys().cloned().collect();
        for path in session_paths {
            self.with_session_mut(&path, |entry, this| {
                entry.session.index_tasks.retain(|&idx, _| idx > index);
                this.request_tasks_entry(entry);
            });
        }
    }

    pub(super) fn on_relay_result(&mut self, result: &RelayResult) {
        if result.result != VerifyResult::Invalid || result.inventory_type != InventoryType::Block {
            return;
        }

        self.on_invalid_block(&result.hash, result.block_index);
    }

    pub(super) fn on_invalid_block(&mut self, hash: &UInt256, block_index: Option<u32>) {
        let mut offenders = Vec::with_capacity(self.sessions.len());

        for entry in self.sessions.values() {
            let mut matches = false;
            if let Some(index) = block_index {
                if let Some(stored) = entry.session.received_block.get(&index) {
                    matches = match block_matches_hash(stored, hash) {
                        BlockHashMatch::Matches => true,
                        BlockHashMatch::DoesNotMatch => false,
                        BlockHashMatch::Unhashable(error) => {
                            warn!(
                                target: "neo",
                                actor = %entry.actor.path(),
                                block_index = index,
                                %error,
                                "treating unhashable cached block as invalid-block offender"
                            );
                            true
                        }
                    };
                }
            } else {
                for stored in entry.session.received_block.values() {
                    match block_matches_hash(stored, hash) {
                        BlockHashMatch::Matches => {
                            matches = true;
                            break;
                        }
                        BlockHashMatch::DoesNotMatch => {}
                        BlockHashMatch::Unhashable(error) => {
                            warn!(
                                target: "neo",
                                actor = %entry.actor.path(),
                                %error,
                                "treating unhashable cached block as invalid-block offender"
                            );
                            matches = true;
                            break;
                        }
                    }
                }
            }

            if matches {
                offenders.push(entry.actor.clone());
            }
        }

        for actor in offenders {
            if let Err(error) = disconnect_peer(&actor, "invalid block relayed".to_string()) {
                warn!(
                    target: "neo",
                    actor = %actor.path(),
                    %error,
                    "failed to disconnect peer carrying invalid block"
                );
            }
        }

        self.request_tasks_all();
    }
}
