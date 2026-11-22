use super::*;
impl AiAgentRuntime {
    pub async fn apply_action(&self, request: OperationalActionRequest) -> ActionResult {
        if !matches!(
            request.action,
            OperationalAction::IncreaseSampling
                | OperationalAction::QuarantineTransaction
                | OperationalAction::ReleaseTransaction
                | OperationalAction::RestartAiWorker
        ) {
            // The rejection itself must be durably audited: an audit append
            // failure can never be downgraded to a clean Unsupported result.
            if self
                .audit_action(&request, &[], "unsupported", false)
                .await
                .is_err()
            {
                return ActionResult::Failed;
            }
            return ActionResult::Unsupported;
        }

        let records = match self.authorize_evidence(&request) {
            Ok(records) => records,
            Err(decision) => {
                // An unauditable rejection must surface as Failed instead of a
                // clean Rejected outcome.
                if self
                    .audit_action(&request, &[], decision_code(decision), false)
                    .await
                    .is_err()
                {
                    return ActionResult::Failed;
                }
                return ActionResult::Rejected(decision);
            }
        };
        let mut validated_request = request.clone();
        for (reference, record) in validated_request.evidence.iter_mut().zip(&records) {
            reference.authority = record.authority;
        }
        let decision = OperationalPolicy::new(self.inner.config.policy).evaluate(
            &validated_request,
            OperationalSafetyState::default(),
            &records,
        );
        if decision != OperationalDecision::Execute {
            let outcome = decision_code(decision);
            // Policy rejections (including TransactionQuarantineDisabled) must
            // still be durably audited; an audit append failure is a Failed
            // outcome rather than a silently unauditable rejection.
            if self
                .audit_action(&validated_request, &records, outcome, false)
                .await
                .is_err()
            {
                return ActionResult::Failed;
            }
            return if decision == OperationalDecision::ObserveOnly {
                ActionResult::Observed
            } else {
                ActionResult::Rejected(decision)
            };
        }
        if let Err(decision) = self.revalidate_action_context(&validated_request, &records) {
            if self
                .audit_action(&validated_request, &records, decision_code(decision), false)
                .await
                .is_err()
            {
                return ActionResult::Failed;
            }
            return ActionResult::Rejected(decision);
        }

        let operation_id = if let Some(store) = &self.inner.action_store {
            let Ok(binding) = self.durable_action_binding(&validated_request, &records) else {
                return ActionResult::Failed;
            };
            let operation_id = binding.operation_id.clone();
            match store.admit(binding, unix_time_ms()) {
                Ok(AdmitOutcome::Admitted(_)) => {}
                Ok(AdmitOutcome::Existing(existing)) => match existing.state {
                    ActionState::Completed => return ActionResult::Executed,
                    ActionState::Admitted => {}
                    ActionState::Running
                    | ActionState::Uncertain
                    | ActionState::ReconciliationRequired
                    | ActionState::Rejected => return ActionResult::Failed,
                },
                Err(_) => return ActionResult::Failed,
            }
            Some(operation_id)
        } else {
            None
        };

        // Authorization must be durably audited before any effect is allowed.
        if self
            .audit_action(&validated_request, &records, "authorized", false)
            .await
            .is_err()
        {
            if let (Some(store), Some(operation_id)) = (&self.inner.action_store, &operation_id) {
                let _ = store.reject(operation_id, "pre_audit_failed", unix_time_ms());
            }
            return ActionResult::Failed;
        }
        if let Err(decision) = self.revalidate_action_context(&validated_request, &records) {
            if let (Some(store), Some(operation_id)) = (&self.inner.action_store, &operation_id) {
                let _ = store.reject(operation_id, "effect_rejected", unix_time_ms());
            }
            if self
                .audit_action(&validated_request, &records, decision_code(decision), false)
                .await
                .is_err()
            {
                return ActionResult::Failed;
            }
            return ActionResult::Rejected(decision);
        }
        if let (Some(store), Some(operation_id)) = (&self.inner.action_store, &operation_id)
            && store.mark_running(operation_id, unix_time_ms()).is_err()
        {
            return ActionResult::Failed;
        }

        let result = match validated_request.action {
            OperationalAction::IncreaseSampling => {
                let current = self.inner.sampling_interval_ms.load(Ordering::Acquire);
                self.inner
                    .sampling_interval_ms
                    .store((current / 2).max(100), Ordering::Release);
                self.inner.sampling_changed.notify_one();
                ActionResult::Executed
            }
            OperationalAction::QuarantineTransaction => self.quarantine(&validated_request),
            OperationalAction::ReleaseTransaction => self.release(&validated_request),
            OperationalAction::RestartAiWorker => {
                self.request_worker_restart();
                ActionResult::Executed
            }
            OperationalAction::StartProbeRequest | OperationalAction::RestartConsensus => {
                ActionResult::Unsupported
            }
        };
        let effect_applied = result == ActionResult::Executed;
        let outcome = if effect_applied { "executed" } else { "failed" };
        if self
            .audit_action(&validated_request, &records, outcome, effect_applied)
            .await
            .is_err()
        {
            if let (Some(store), Some(operation_id)) = (&self.inner.action_store, &operation_id) {
                if effect_applied {
                    let _ = store.mark_reconciliation_required(operation_id, unix_time_ms());
                } else {
                    let _ = store.reject(operation_id, "effect_rejected", unix_time_ms());
                }
            }
            return ActionResult::Failed;
        }
        if let (Some(store), Some(operation_id)) = (&self.inner.action_store, &operation_id) {
            let persisted = if effect_applied {
                store.mark_completed(operation_id, unix_time_ms())
            } else {
                store.reject(operation_id, "effect_rejected", unix_time_ms())
            };
            if persisted.is_err() {
                return ActionResult::Failed;
            }
        }
        result
    }

    // Node integration tests exercise this legacy internal contract cross-crate.
    #[doc(hidden)]
    #[allow(clippy::result_unit_err)]
    pub fn durable_action_binding(
        &self,
        request: &OperationalActionRequest,
        records: &[EvidenceRecord],
    ) -> Result<ActionBinding, ()> {
        let rule_version = records
            .first()
            .map(|record| record.rule_version.as_str())
            .ok_or(())?;
        if records
            .iter()
            .any(|record| record.rule_version != rule_version)
        {
            return Err(());
        }
        let material = json!({
            "action": request.action,
            "target": request.target,
            "evidence": records.iter().map(|record| (&record.id, &record.content_hash)).collect::<Vec<_>>(),
            "rule_version": rule_version,
        });
        let bytes = serde_json::to_vec(&material).map_err(|_| ())?;
        let hash = hex::encode(Sha256::digest(bytes).as_slice());
        let operation_id = OperationId::new(format!("action-{}", &hash[..40])).map_err(|_| ())?;
        ActionBinding::new(
            operation_id,
            request.action,
            request.target.clone(),
            records.iter().map(|record| record.id.clone()).collect(),
            records
                .iter()
                .map(|record| record.content_hash.clone())
                .collect(),
            rule_version.to_owned(),
        )
        .map_err(|_| ())
    }

    fn revalidate_action_context(
        &self,
        request: &OperationalActionRequest,
        records: &[EvidenceRecord],
    ) -> Result<(), OperationalDecision> {
        if request.action != OperationalAction::QuarantineTransaction {
            return Ok(());
        }
        let record = records
            .iter()
            .find(|record| record.kind == EvidenceKind::ProbeRuleFinding)
            .ok_or(OperationalDecision::MissingProbeRuleFinding)?;
        if record.authority != EvidenceAuthority::DeterministicRule
            || record.source != EvidenceSource::DeterministicDetector
            || record.rule_version != PROBE_DETERMINISTIC_RULE_VERSION
        {
            return Err(OperationalDecision::EvidenceRuleMismatch);
        }
        let fields = record.item.fields();
        let context_hash = fields
            .get("context_hash")
            .and_then(Value::as_str)
            .ok_or(OperationalDecision::InvalidEvidence)?;
        let snapshot_height = fields
            .get("snapshot_height")
            .and_then(Value::as_u64)
            .and_then(|height| u32::try_from(height).ok())
            .ok_or(OperationalDecision::InvalidEvidence)?;
        let snapshot_root = fields
            .get("snapshot_root")
            .and_then(Value::as_str)
            .ok_or(OperationalDecision::InvalidEvidence)?;
        if !valid_hash(context_hash)
            || snapshot_height != record.observed_height
            || snapshot_height != record.tip_height
            || record.tip_hash.as_deref() != Some(snapshot_root)
        {
            return Err(OperationalDecision::InvalidEvidence);
        }
        let current_height = self.inner.host.chain.current_block_index();
        if current_height != snapshot_height
            || self
                .inner
                .host
                .chain
                .block_hash_at(current_height)
                .is_none_or(|hash| hash.canonical != snapshot_root)
        {
            return Err(OperationalDecision::StaleEvidence);
        }
        Ok(())
    }

    fn authorize_evidence(
        &self,
        request: &OperationalActionRequest,
    ) -> Result<Vec<EvidenceRecord>, OperationalDecision> {
        let registry = self.inner.registry.lock();
        let mut records = Vec::with_capacity(request.evidence.len());
        for reference in &request.evidence {
            let record = registry
                .get(&reference.id)
                .ok_or(OperationalDecision::InvalidEvidence)?;
            if record.content_hash != reference.content_hash
                || record.authority != reference.authority
            {
                return Err(OperationalDecision::InvalidEvidence);
            }
            self.validate_record(record, true)?;
            records.push(record.clone());
        }
        if matches!(
            request.action,
            OperationalAction::QuarantineTransaction | OperationalAction::ReleaseTransaction
        ) {
            let target = request
                .target
                .as_deref()
                .filter(|target| !target.is_empty())
                .ok_or(OperationalDecision::MissingTarget)?;
            if !records.iter().any(|record| {
                record.authority == EvidenceAuthority::DeterministicRule
                    && record.transaction_hash.as_deref() == Some(target)
            }) {
                return Err(OperationalDecision::EvidenceTargetMismatch);
            }
        }
        Ok(records)
    }

    pub(super) fn validate_record(
        &self,
        record: &EvidenceRecord,
        allow_deterministic: bool,
    ) -> Result<(), OperationalDecision> {
        if record.verify_integrity().is_err()
            || (!allow_deterministic && record.authority == EvidenceAuthority::DeterministicRule)
            || record.kind == EvidenceKind::ProbeResult
            || record.source == EvidenceSource::RestrictedProbe
        {
            // Probe results must arrive through the future opaque parent-validated path,
            // never through this generic evidence ingress.
            return Err(OperationalDecision::InvalidEvidence);
        }
        let expected_rule_version = if record.kind == EvidenceKind::ProbeRuleFinding {
            PROBE_DETERMINISTIC_RULE_VERSION
        } else {
            self.inner.config.rule_version.as_str()
        };
        if record.rule_version != expected_rule_version {
            return Err(OperationalDecision::EvidenceRuleMismatch);
        }
        if record.kind == EvidenceKind::ProbeRuleFinding
            && (record.authority != EvidenceAuthority::DeterministicRule
                || record.source != EvidenceSource::DeterministicDetector)
        {
            return Err(OperationalDecision::InvalidEvidence);
        }
        let now = unix_time_ms();
        let current_height = self.inner.host.chain.current_block_index();
        if record.observed_at_unix_ms > now
            || now.saturating_sub(record.observed_at_unix_ms) > self.inner.config.event_freshness_ms
            || record.tip_height > current_height
            || record.observed_height > record.tip_height.saturating_add(1)
        {
            return Err(OperationalDecision::StaleEvidence);
        }
        if let Some(expected_tip_hash) = record.tip_hash.as_deref()
            && self
                .inner
                .host
                .chain
                .block_hash_at(record.tip_height)
                .is_none_or(|hash| {
                    // Generic ingress records carry the `0x`-prefixed display
                    // form, while probe rule findings carry the canonical
                    // unprefixed 64-hex form; both must match the same block.
                    hash.display != expected_tip_hash && hash.canonical != expected_tip_hash
                })
        {
            return Err(OperationalDecision::InvalidEvidence);
        }
        Ok(())
    }

    /// Resolves an action target hash to the mempool transaction hash.
    ///
    /// Targets reach the quarantine chain in two hex conventions: the
    /// conventional display form (`UInt256::to_string`, reversed byte order,
    /// optional `0x` prefix) used by generic ingress evidence, and the
    /// canonical natural-order sha256 hex the probe contract binds to
    /// `transaction_bytes`. `UInt256::parse` only understands the display
    /// form, so the natural-order reading is tried as well and wins only
    /// when it names a transaction that is verified in the mempool or
    /// already quarantined; otherwise the display reading is authoritative.
    pub(super) fn resolve_target_hash(&self, target: &str) -> Option<String> {
        self.inner
            .host
            .mempool
            .resolve_target(target, unix_time_ms())
    }

    fn quarantine(&self, request: &OperationalActionRequest) -> ActionResult {
        let Some(target) = request.target.as_deref() else {
            return ActionResult::InvalidTarget;
        };
        let Some(transaction_hash) = self.resolve_target_hash(target) else {
            return ActionResult::InvalidTarget;
        };
        if self.is_quarantined_target(request) {
            // The transaction is already actively contained: repeated trusted
            // probe verdicts stay idempotent instead of failing with the
            // mempool's AlreadyActive error.
            return ActionResult::Executed;
        }
        let now = unix_time_ms();
        let metadata = QuarantineMetadata {
            transaction_hash,
            evidence_refs: request
                .evidence
                .iter()
                .map(|item| item.id.clone())
                .collect(),
            context: "trusted double replay fixed probe rule".to_string(),
            created_at_unix_ms: now,
            expires_at_unix_ms: now.saturating_add(self.inner.config.quarantine_ttl_ms),
            rule_version: PROBE_DETERMINISTIC_RULE_VERSION.to_string(),
            episode: 0,
            cumulative_active_ms: 0,
        };
        self.inner
            .host
            .mempool
            .quarantine(metadata, now)
            .map_or(ActionResult::Failed, |()| ActionResult::Executed)
    }

    fn release(&self, request: &OperationalActionRequest) -> ActionResult {
        let Some(target) = request.target.as_deref() else {
            return ActionResult::InvalidTarget;
        };
        let Some(transaction_hash) = self.resolve_target_hash(target) else {
            return ActionResult::InvalidTarget;
        };
        self.inner.host.mempool.release(&transaction_hash);
        ActionResult::Executed
    }
}
