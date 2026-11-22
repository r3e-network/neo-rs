use super::*;

use super::policy::{
    build_proposal_policy, conflicts_with_context, extract_signature, signer_accounts,
};
use super::recovery::apply_proposal_verdict;

use neo_consensus::{
    BlockLimitVerdict, ChangeViewReason, CommitEgressGuard, ConsensusPayload,
    PrepareRequestPrecheck, ProposalCandidateSource, ProposalTransactionCheck,
    ProposalTransactionVerdict, RejectionCause, TransactionSummary, TransactionSummarySource,
    VerificationOutcome, precheck_prepare_request,
};
use neo_crypto::MerkleTree;
use neo_io::Serializable;
use neo_ledger::{TransactionVerificationContext, verify_transaction};
use neo_p2p::p2p::payloads::InvPayload;
use neo_primitives::{ContainsTransactionType, InventoryType, VerifyResult};
use neo_protocol::payloads::extensible::Extensible as ExtensiblePayload;
use neo_protocol::payloads::{Block, Header, Witness};
use neo_vm::ScriptBuilder;
#[cfg(feature = "tee")]
use parking_lot::Mutex;
use tracing::{debug, error, warn};

/// Node adapter for [`ProposalCandidateSource`].
///
/// Owns the concrete mempool/TEE ordering (the consensus crate only sees
/// transaction summaries) and remembers the transactions it handed out so the
/// actor can rebuild the selected block by hash.
struct MempoolCandidateSource {
    system: Arc<neo_system::NeoSystem>,
    by_hash: HashMap<UInt256, Arc<Transaction>>,
}

impl MempoolCandidateSource {
    fn new(system: Arc<neo_system::NeoSystem>) -> Self {
        Self {
            system,
            by_hash: HashMap::new(),
        }
    }

    fn transaction(&self, hash: &UInt256) -> Option<Arc<Transaction>> {
        self.by_hash.get(hash).cloned()
    }

    fn ordered_transactions(&mut self, limit: usize) -> Vec<Arc<Transaction>> {
        if limit == 0 {
            return Vec::new();
        }

        let pool = self.system.mempool();

        // Privacy mode (strictly opt-in): the active TEE runtime has content
        // encryption enabled. Resolve pending ciphertexts through the TEE
        // decrypt state machine first — only Encrypted→Resolved entries enter
        // the candidate ordering; Rejected entries fail closed and are logged
        // and counted instead. The TEE ordered hash table is then intersected
        // with the local isolation view before any candidate can be proposed.
        #[cfg(feature = "tee")]
        if let Some(runtime) = crate::tee_integration::active_runtime()
            && runtime.privacy_enabled()
        {
            let summary = runtime.mempool.resolve_pending();
            if summary.rejected > 0 {
                warn!(
                    target: "neo::consensus",
                    rejected = summary.rejected,
                    "privacy mode: rejected encrypted mempool entries are excluded from proposal candidates"
                );
            }
            return Self::tee_ordered_candidates(&runtime, &pool, limit);
        }

        pool.lock().get_sorted_verified_transactions(limit)
    }

    /// TEE ordered hash table intersected with the bounded local-only
    /// isolation view. Network receive and external proposal paths never call
    /// this API.
    #[cfg(feature = "tee")]
    fn tee_ordered_candidates(
        runtime: &crate::tee_integration::TeeRuntime,
        pool: &std::sync::Arc<Mutex<neo_mempool::MemoryPool>>,
        limit: usize,
    ) -> Vec<Arc<Transaction>> {
        let tee_hashes = runtime
            .mempool
            .get_ordered_hashes(limit.saturating_mul(4).max(limit));
        let mut pool_guard = pool.lock();
        let locally_allowed = pool_guard.local_proposal_transactions(
            limit.saturating_mul(4).max(limit),
            ConsensusActor::current_time_ms(),
        );

        let mut verified_by_hash = HashMap::new();
        for tx in &locally_allowed {
            verified_by_hash.insert(tx.hash(), Arc::clone(tx));
        }

        let mut candidates = Vec::with_capacity(limit);
        let mut seen = HashSet::with_capacity(limit.saturating_mul(2));

        for hash_bytes in tee_hashes {
            if candidates.len() >= limit {
                break;
            }

            let Ok(hash) = UInt256::from_bytes(&hash_bytes) else {
                continue;
            };
            let Some(tx) = verified_by_hash.get(&hash) else {
                continue;
            };

            let tx_hash = tx.hash();
            if seen.insert(tx_hash) {
                candidates.push(Arc::clone(tx));
            }
        }

        if candidates.len() < limit {
            for tx in locally_allowed {
                let tx_hash = tx.hash();
                if seen.insert(tx_hash) {
                    candidates.push(tx);
                }
                if candidates.len() >= limit {
                    break;
                }
            }
        }

        candidates
    }
}

impl TransactionSummarySource for MempoolCandidateSource {
    fn transaction_summary(&self, hash: &UInt256) -> Option<TransactionSummary> {
        self.by_hash
            .get(hash)
            .map(|tx| TransactionSummary::new(tx.hash(), tx.size(), tx.system_fee()))
    }
}

impl ProposalCandidateSource for MempoolCandidateSource {
    fn proposal_candidate_hashes(&mut self, limit: usize) -> Vec<UInt256> {
        let transactions = self.ordered_transactions(limit);
        let hashes: Vec<UInt256> = transactions.iter().map(|tx| tx.hash()).collect();
        self.by_hash = transactions.into_iter().map(|tx| (tx.hash(), tx)).collect();
        hashes
    }
}

impl ConsensusActor {
    pub(super) fn authorize_consensus_payload(&self, payload: &ConsensusPayload) -> bool {
        let authorized = CommitEgressGuard::authorize_payload(payload, self.my_index, self);
        if !authorized {
            error!(target: "neo", validator = payload.validator_index, "local Commit persistence authorization failed; withholding broadcast");
        }
        authorized
    }

    pub(super) fn authorize_block_commit(&self, block_data: &BlockData) -> bool {
        let authorized = CommitEgressGuard::authorize_block(block_data, self.my_index, self);
        if !authorized {
            error!(target: "neo", validator = ?self.my_index, "final block local Commit persistence authorization failed; withholding block");
        }
        authorized
    }

    pub(super) fn build_extensible_payload(
        &self,
        payload: &ConsensusPayload,
    ) -> Option<ExtensiblePayload> {
        let validator = self
            .validators
            .get(payload.validator_index as usize)?
            .clone();
        let sender = Contract::create_signature_contract(validator.clone()).script_hash();
        let signature = payload.witness.clone();
        let mut builder = ScriptBuilder::new();
        builder.emit_push(&signature);
        let invocation = builder.to_array();
        let verification = Contract::create_signature_redeem_script(validator);
        let witness = Witness::new_with_scripts(invocation, verification);

        let mut extensible = ExtensiblePayload::new();
        extensible.category = "dBFT".to_string();
        extensible.valid_block_start = 0;
        extensible.valid_block_end = payload.block_index;
        extensible.sender = sender;
        extensible.data = payload.to_message_bytes();
        extensible.witness = witness;
        Some(extensible)
    }

    pub(super) fn propose_transactions(&mut self, max_count: usize) {
        if self.service.is_none() {
            return;
        }

        let policy = build_proposal_policy(&self.system, &self.settings, &self.validators);
        let rejected: HashSet<UInt256> = self
            .service
            .as_ref()
            .map(|service| {
                service
                    .context()
                    .rejected_hashes_above_f()
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default();

        let mut source = MempoolCandidateSource::new(self.system.clone());
        // Skip transactions already rejected by >F validators; re-proposing
        // them only burns views (C# InvalidTransactions).
        let selection = policy.select_from(&mut source, max_count, &rejected);
        if selection.skipped_rejected > 0 {
            debug!(
                target: "neo::consensus",
                skipped_rejected = selection.skipped_rejected,
                "proposal selection skipped validator-rejected transactions"
            );
        }

        // Convert Arc<Transaction> to Transaction for proposal; the policy kept
        // only summaries, so the concrete transactions are looked up by hash.
        self.proposal_transactions = selection
            .hashes()
            .into_iter()
            .filter_map(|hash| source.transaction(&hash).map(|tx| (hash, (*tx).clone())))
            .collect();

        let hashes = selection.hashes();
        if let Some(service) = self.service.as_mut()
            && let Err(err) = service.on_transactions_received(hashes)
        {
            warn!(target: "neo", %err, "failed to submit proposal transactions");
        }
    }

    pub(super) fn try_prepare_response(&mut self, now: u64) {
        let Some(service) = self.service.as_mut() else {
            return;
        };

        if service.context().is_primary() {
            return;
        }

        let Some(my_index) = self.my_index else {
            return;
        };

        if service.context().prepare_responses.contains_key(&my_index) {
            return;
        }

        if !service.context().prepare_request_received {
            return;
        }

        let proposed_hashes = service.context().proposed_tx_hashes.clone();
        if proposed_hashes.is_empty() {
            let _ = service.on_transactions_received(Vec::new());
            return;
        }

        let settings = self.system.settings();
        let policy = build_proposal_policy(&self.system, &self.settings, &self.validators);
        let store_cache = self.system.store_cache();
        let snapshot = store_cache.data_cache();
        let pool = self.system.mempool();
        let pool_guard = pool.lock();
        let verified_map: HashMap<UInt256, Transaction> = pool_guard
            .verified_transactions_vec()
            .into_iter()
            .map(|arc_tx| ((*arc_tx).clone().hash(), (*arc_tx).clone()))
            .collect();
        let unverified_map: HashMap<UInt256, Transaction> = pool_guard
            .unverified_transactions_vec()
            .into_iter()
            .map(|arc_tx| ((*arc_tx).clone().hash(), (*arc_tx).clone()))
            .collect();
        drop(pool_guard);

        let mut verification_context = TransactionVerificationContext::new();
        let mut transactions = HashMap::new();
        let mut missing = HashSet::new();

        for hash in &proposed_hashes {
            let already_on_chain = self.system.context().contains_transaction(hash)
                == ContainsTransactionType::ExistsInLedger;

            let Some(tx) = verified_map.get(hash).or_else(|| unverified_map.get(hash)) else {
                if already_on_chain {
                    apply_proposal_verdict(
                        service,
                        hash,
                        ProposalTransactionVerdict::Reject {
                            reason: ChangeViewReason::TxInvalid,
                            cause: RejectionCause::AlreadyOnChain,
                        },
                        now,
                    );
                    return;
                }
                missing.insert(*hash);
                continue;
            };
            let already_verified = verified_map.contains_key(hash);

            let mut check = ProposalTransactionCheck {
                already_on_chain,
                conflicts_with_ledger: self
                    .system
                    .context()
                    .contains_conflict_hash(hash, &signer_accounts(tx)),
                conflicts_with_proposal: conflicts_with_context(tx, &transactions),
                system_fee: tx.system_fee(),
                verification: VerificationOutcome::Succeeded,
            };

            // Ledger/proposal/fee checks run before deterministic verification,
            // matching the reference ordering.
            if apply_proposal_verdict(service, hash, policy.evaluate_transaction(&check), now) {
                return;
            }

            if !already_verified {
                let result =
                    verify_transaction(tx, settings, snapshot, Some(&verification_context), &[]);
                if result != VerifyResult::Succeed {
                    warn!(
                        target: "neo::consensus",
                        block_index = service.context().block_index,
                        tx_hash = %hash,
                        validation_result = ?result,
                        path = "prepare_response",
                        "deterministic transaction validation rejected proposal"
                    );
                }
                check.verification = if result == VerifyResult::Succeed {
                    VerificationOutcome::Succeeded
                } else if result == VerifyResult::PolicyFail {
                    VerificationOutcome::RejectedByPolicy
                } else {
                    VerificationOutcome::Invalid
                };
                if apply_proposal_verdict(service, hash, policy.evaluate_transaction(&check), now) {
                    return;
                }
            }

            transactions.insert(*hash, tx.clone());
            verification_context.add_transaction(tx);
        }

        if !missing.is_empty() {
            let should_request = self
                .missing_transactions
                .as_ref()
                .is_none_or(|prev| prev != &missing);
            if should_request {
                let hashes: Vec<UInt256> = missing.iter().copied().collect();
                let inv = InvPayload::create(InventoryType::Transaction, &hashes);
                if let Err(err) = self
                    .system
                    .task_manager_handle()
                    .broadcast_restart_tasks(inv)
                {
                    debug!(target: "neo", %err, "failed to request missing transactions");
                }
                self.missing_transactions = Some(missing);
            }
            return;
        }

        let total_system_fee = transactions
            .values()
            .fold(0i64, |acc, tx| acc.saturating_add(tx.system_fee()));
        let total_tx_size = transactions
            .values()
            .map(neo_io::Serializable::size)
            .sum::<usize>();
        if let BlockLimitVerdict::Reject { reason } =
            policy.evaluate_block(transactions.len(), total_tx_size, total_system_fee)
        {
            let _ = service.request_change_view(reason, now);
            return;
        }

        self.proposal_transactions = transactions;
        self.missing_transactions = None;
        let _ = service.on_transactions_received(proposed_hashes);
    }

    pub(super) fn try_finalize_pending_block(&mut self, _now: u64) {
        let Some(block_data) = self.pending_block.clone() else {
            return;
        };

        let current_index = self.current_block_index.unwrap_or_default();
        if block_data.block_index != current_index {
            self.pending_block = None;
            return;
        }

        let mut missing = Vec::new();
        let mut transactions = Vec::with_capacity(block_data.transaction_hashes.len());

        for hash in &block_data.transaction_hashes {
            if let Some(tx) = self.proposal_transactions.get(hash) {
                transactions.push(tx.clone());
                continue;
            }
            if let Some(tx) = self.system.context().try_get_transaction_from_mempool(hash) {
                transactions.push(tx);
                continue;
            }
            missing.push(*hash);
        }

        if !missing.is_empty() {
            let inv = InvPayload::create(InventoryType::Transaction, &missing);
            if let Err(err) = self
                .system
                .task_manager_handle()
                .broadcast_restart_tasks(inv)
            {
                debug!(target: "neo", %err, "failed to request block transactions");
            }
            return;
        }

        let Some(prev_hash) = self.current_prev_hash else {
            warn!(target: "neo", "missing prev_hash for consensus block");
            return;
        };

        // `compute_root` yields `None` only for an empty hash set. Handle that
        // case explicitly instead of silently defaulting the root: an empty
        // proposal keeps the zero merkle root, matching the consensus-side
        // block builder and `Block::try_rebuild_merkle_root`. The debug
        // assertion pins the invariant so a future non-empty `None` path fails
        // loudly instead of persisting a header with a fabricated root.
        let merkle_root = match MerkleTree::compute_root(&block_data.transaction_hashes) {
            Some(root) => root,
            None => {
                debug_assert!(block_data.transaction_hashes.is_empty());
                debug!(
                    target: "neo",
                    block_index = block_data.block_index,
                    "finalizing an empty-proposal block with the zero merkle root"
                );
                UInt256::zero()
            }
        };
        let next_consensus = NativeHelpers::get_bft_address(&block_data.validator_pubkeys);

        let mut header = Header::new();
        header.set_version(0);
        header.set_prev_hash(prev_hash);
        header.set_merkle_root(merkle_root);
        header.set_timestamp(block_data.timestamp);
        header.set_nonce(block_data.nonce);
        header.set_index(block_data.block_index);
        header.set_primary_index(block_data.primary_index);
        header.set_next_consensus(next_consensus);

        let contract = Contract::create_multi_sig_contract(
            block_data.required_signatures,
            &block_data.validator_pubkeys,
        );
        let mut witness_builder =
            match neo_wallets::MultiSigWitnessBuilder::from_contract(&contract) {
                Ok(builder) => builder,
                Err(err) => {
                    warn!(target: "neo", %err, "failed to initialize block witness");
                    return;
                }
            };
        for (validator_index, signature) in &block_data.signatures {
            let Some(pubkey) = block_data.validator_pubkeys.get(*validator_index as usize) else {
                warn!(target: "neo", validator_index, "commit references an unknown validator");
                return;
            };
            let encoded = match pubkey.encode_point(true) {
                Ok(encoded) => encoded,
                Err(err) => {
                    warn!(target: "neo", %err, validator_index, "invalid validator public key");
                    return;
                }
            };
            match witness_builder.add_signature(&encoded, signature.clone()) {
                Ok(true) => {}
                Ok(false) => {
                    warn!(target: "neo", validator_index, "commit key is absent from the block contract");
                    return;
                }
                Err(err) => {
                    warn!(target: "neo", %err, validator_index, "invalid commit signature");
                    return;
                }
            }
        }

        header.witness = match witness_builder.build_witness() {
            Ok(witness) => witness,
            Err(err) => {
                warn!(target: "neo", %err, "failed to build block witness from commits");
                return;
            }
        };

        let mut block = Block {
            header,
            transactions,
        };
        block.rebuild_merkle_root();

        if let Err(err) =
            self.system
                .blockchain_actor()
                .tell(neo_ledger::BlockchainCommand::InventoryBlock {
                    block: Arc::new(neo_protocol::execution::Block::from(&block)),
                    relay: true,
                })
        {
            warn!(target: "neo", %err, "failed to submit consensus block");
            return;
        }

        self.pending_block = None;
    }

    pub(super) fn on_relay_result(&mut self, result: neo_p2p::p2p::ports::RelayOutcome) {
        if result.result != VerifyResult::Succeed {
            return;
        }

        if result.inventory_type == InventoryType::Transaction {
            let waiting_on_tx = self
                .missing_transactions
                .as_ref()
                .is_some_and(|missing| missing.contains(&result.hash))
                || self
                    .pending_block
                    .as_ref()
                    .is_some_and(|block| block.transaction_hashes.contains(&result.hash));
            if waiting_on_tx {
                let now = Self::current_time_ms();
                self.try_prepare_response(now);
                self.try_finalize_pending_block(now);
            }
            return;
        }

        if result.inventory_type != InventoryType::Extensible {
            return;
        }

        let context = self.system.context();
        let payload = context
            .try_get_relay_extensible(&result.hash)
            .or_else(|| context.try_get_extensible(&result.hash));
        if let Some(payload) = payload {
            self.on_extensible_payload(payload);
        }
    }

    fn on_extensible_payload(&mut self, payload: ExtensiblePayload) {
        if payload.category != "dBFT" || payload.data.is_empty() {
            return;
        }

        let Some(signature) = extract_signature(&payload.witness) else {
            return;
        };

        let network = self.system.settings().network;
        let consensus_payload =
            match ConsensusPayload::from_message_bytes(network, &payload.data, signature) {
                Ok(value) => value,
                Err(err) => {
                    debug!(target: "neo", %err, "failed to parse consensus payload");
                    return;
                }
            };

        if !self.precheck_prepare_request(&consensus_payload) {
            return;
        }

        let Some(service) = self.service.as_mut() else {
            return;
        };

        if let Err(err) = service.process_message(consensus_payload) {
            debug!(target: "neo", %err, "consensus message rejected");
            return;
        }

        self.try_prepare_response(Self::current_time_ms());
    }

    fn precheck_prepare_request(&self, payload: &ConsensusPayload) -> bool {
        let check = PrepareRequestPrecheck {
            payload,
            expected_block_index: self.current_block_index,
            expected_prev_hash: self.current_prev_hash,
            prev_timestamp: self.prev_timestamp.unwrap_or(0),
            now_ms: Self::current_time_ms(),
            time_per_block_ms: self.system.time_per_block().as_millis() as u64,
            max_transactions_per_block: self.system.settings().max_transactions_per_block as usize,
        };
        match precheck_prepare_request(&check) {
            Ok(()) => true,
            Err(rejection) => {
                debug!(target: "neo", ?rejection, "prepare request screened out");
                false
            }
        }
    }
}
