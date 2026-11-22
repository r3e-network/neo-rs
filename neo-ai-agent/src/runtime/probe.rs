use super::*;
impl AiAgentRuntime {
    /// Consumes a `restricted_probe_candidate` finding through the controlled
    /// probe submission path (plan §38): a probe job is constructed from the
    /// finding and the startup-validated restricted probe configuration, then
    /// offered to the controller's bounded `try_submit`. Every failure mode
    /// (no controller behind the fail-closed gates, unavailable transaction
    /// bytes, full queue, admission cooldown) is recorded and never panics.
    pub(super) fn handle_restricted_probe_candidate(&self, candidate: &EvidenceRecord) {
        self.inner.probe_candidates.fetch_add(1, Ordering::Relaxed);
        let config = self.inner.probe_config.lock().clone();
        let Some(config) = config else {
            // The fail-closed startup gates never validated a restricted
            // probe configuration: record the skip and consume the candidate.
            self.inner
                .probe_submission_skips
                .fetch_add(1, Ordering::Relaxed);
            return;
        };
        let Some(job) = self.probe_candidate_job(candidate, &config) else {
            // The transaction bytes are unavailable (for example the block is
            // no longer retrievable): record the skip without panicking.
            self.inner
                .probe_submission_skips
                .fetch_add(1, Ordering::Relaxed);
            return;
        };
        self.submit_probe_candidate_job(job);
    }

    /// Builds the controlled probe job for a reproducible-execution finding.
    /// The transaction hash is bound to the finding, the replay context is
    /// the live chain tip, and the budget comes exclusively from the
    /// startup-validated restricted probe configuration.
    fn probe_candidate_job(
        &self,
        candidate: &EvidenceRecord,
        config: &ProbeSessionConfig,
    ) -> Option<ProbeJob> {
        let transaction_hash = candidate.transaction_hash.clone()?;
        let transaction = self
            .inner
            .host
            .chain
            .unsigned_transaction(&transaction_hash, candidate.observed_height)?;
        let height = self.inner.host.chain.current_block_index();
        let tip_hash = self
            .inner
            .host
            .chain
            .block_hash_at(height)
            .map(|hash| hash.canonical)?;
        let now = unix_time_ms();
        let fingerprint = hex::encode(Sha256::digest(candidate.id.as_bytes()).as_slice());
        let operation_id =
            OperationId::new(format!("probe-candidate-{}", &fingerprint[..24])).ok()?;
        let run_id = RunId::new(format!("probe-run-{}", &fingerprint[24..40])).ok()?;
        let spec = PROBE_ACTION_CATALOG
            .iter()
            .find(|entry| entry.action == ProbeAction::TransactionPreflight)?;
        // The budget is clamped to the fixed probe contract catalog so the
        // startup-validated configuration can never exceed it.
        let budget = ProbeBudget {
            max_duration_ms: config
                .replay_timeout_ms
                .min(spec.maximum_budget.max_duration_ms),
            max_output_bytes: u64::try_from(config.frame_limit_bytes)
                .unwrap_or(0)
                .min(spec.maximum_budget.max_output_bytes),
            max_observations: spec.maximum_budget.max_observations,
        };
        if !valid_hash(&config.worker_binary_hash) {
            return None;
        }
        ProbeJob::new(
            operation_id,
            run_id,
            ProbeAction::TransactionPreflight,
            ProbeTransaction {
                transaction_hash: transaction.hash_hex,
                transaction_bytes: transaction.bytes,
            },
            ProbeContext {
                observed_height: height,
                tip_hash,
                requested_at_unix_ms: now,
                node_instance_id: "neo-node-local-diagnostics".to_owned(),
            },
            ProbeBuild {
                binary_hash: config.worker_binary_hash.clone(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
            },
            budget,
            AuthorizerMetadata {
                evidence_id: candidate.id.clone(),
                evidence_hash: candidate.content_hash.clone(),
                authority: EvidenceAuthority::DeterministicRule,
                rule_version: candidate.rule_version.clone(),
                authorized_at_unix_ms: now,
            },
        )
        .ok()
    }

    /// Offers a prepared probe job to the controller's bounded submission
    /// queue; every bounded rejection is recorded instead of panicking.
    fn submit_probe_candidate_job(&self, job: ProbeJob) {
        self.inner
            .probe_submission_attempts
            .fetch_add(1, Ordering::Relaxed);
        self.inner.probe_submission_log.lock().push(job.clone());
        let Some(control) = &self.inner.probe_controller else {
            // No controller exists behind the fail-closed gates: the prepared
            // job is recorded as a skip and dropped.
            self.inner
                .probe_submission_skips
                .fetch_add(1, Ordering::Relaxed);
            return;
        };
        if control.try_submit(job).is_err() {
            // A full queue, admission cooldown, or closed controller is an
            // expected bounded outcome: record it and keep the worker alive.
            self.inner
                .probe_submission_failures
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Accepts only the controller's completed, parent-validated double-replay result.
    pub async fn apply_probe_completion(&self, completion: ProbeCompletion) -> ActionResult {
        let ProbeCompletion {
            operation_id,
            run_id,
            result,
        } = completion;
        let Some(result) = result else {
            return ActionResult::Rejected(OperationalDecision::InvalidEvidence);
        };
        let fields = result.record().item.fields();
        if fields.get("operation_id").and_then(Value::as_str) != Some(operation_id.as_str())
            || fields.get("run_id").and_then(Value::as_str) != Some(run_id.as_str())
        {
            return ActionResult::Rejected(OperationalDecision::InvalidEvidence);
        }
        // The completion must be backed by the controller's durably completed
        // StartProbeRequest operation before any policy may act on it.
        if let Some(store) = &self.inner.action_store
            && !matches!(
                store.query(&operation_id),
                Ok(Some(record))
                    if record.state == ActionState::Completed
                        && record.binding.action == OperationalAction::StartProbeRequest
            )
        {
            return ActionResult::Rejected(OperationalDecision::InvalidEvidence);
        }
        self.apply_probe_result(result).await
    }

    /// Consumes only an opaque parent-validated probe result, applies the fixed rule,
    /// then delegates any optional local quarantine to the ordinary policy/action path.
    async fn apply_probe_result(&self, result: ProbeResult) -> ActionResult {
        let observation = result.record().clone();
        if self.validate_probe_observation(&observation).is_err() {
            return ActionResult::Rejected(OperationalDecision::InvalidEvidence);
        }
        self.inner.registry.lock().insert(observation);
        let finding = match ProbeDeterministicRule.evaluate(&result) {
            Ok(Some(finding))
                if finding.finding() == ProbeRuleFindingKind::TransactionIsolationCandidate =>
            {
                finding
            }
            Ok(_) => {
                return ActionResult::Rejected(OperationalDecision::MissingProbeRuleFinding);
            }
            Err(_) => return ActionResult::Rejected(OperationalDecision::InvalidEvidence),
        };
        let (record, _) = finding.into_parts();
        let Some(target) = record.transaction_hash.clone() else {
            return ActionResult::Rejected(OperationalDecision::EvidenceTargetMismatch);
        };
        self.inner.registry.lock().insert(record.clone());
        self.apply_action(action_request(
            OperationalAction::QuarantineTransaction,
            Some(target),
            &record,
        ))
        .await
    }

    fn validate_probe_observation(
        &self,
        record: &EvidenceRecord,
    ) -> Result<(), OperationalDecision> {
        if record.verify_integrity().is_err()
            || record.authority != EvidenceAuthority::Observation
            || record.kind != EvidenceKind::ProbeResult
            || record.source != EvidenceSource::RestrictedProbe
            || record.rule_version != self.inner.config.rule_version
        {
            return Err(OperationalDecision::InvalidEvidence);
        }
        let fields = record.item.fields();
        if !valid_hash(
            fields
                .get("context_hash")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ) || fields.get("snapshot_height").and_then(Value::as_u64)
            != Some(u64::from(record.tip_height))
            || fields.get("snapshot_root").and_then(Value::as_str) != record.tip_hash.as_deref()
        {
            return Err(OperationalDecision::InvalidEvidence);
        }
        let now = unix_time_ms();
        let current_height = self.inner.host.chain.current_block_index();
        if record.observed_at_unix_ms > now
            || now.saturating_sub(record.observed_at_unix_ms) > self.inner.config.event_freshness_ms
            || record.tip_height != current_height
            || record.observed_height != current_height
            || record.tip_hash.as_deref()
                != self
                    .inner
                    .host
                    .chain
                    .block_hash_at(current_height)
                    .map(|hash| hash.canonical)
                    .as_deref()
        {
            return Err(OperationalDecision::StaleEvidence);
        }
        Ok(())
    }
}
