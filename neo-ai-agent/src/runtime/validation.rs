use super::*;

use super::evidence::{ExecutionRepetition, QUARANTINE_EVIDENCE_FAILURES, ValidationFailure};

impl AiAgentRuntime {
    pub fn try_submit_record(&self, record: EvidenceRecord) -> EvidenceSubmitResult {
        if !self.inner.config.enabled {
            return EvidenceSubmitResult::Disabled;
        }
        match self.inner.evidence_ingress.try_export(record) {
            ExportResult::Enqueued => EvidenceSubmitResult::Enqueued,
            ExportResult::DroppedFull => EvidenceSubmitResult::DroppedFull,
            ExportResult::DroppedClosed => EvidenceSubmitResult::DroppedClosed,
        }
    }

    pub(super) fn reproducible_execution_candidate(
        &self,
        record: &EvidenceRecord,
    ) -> Option<EvidenceRecord> {
        if record.authority != EvidenceAuthority::Observation
            || record.kind != EvidenceKind::TransactionExecution
            || record.source != EvidenceSource::NodeRuntime
        {
            return None;
        }
        let fields = record.item.fields();
        if fields.get("vm_state").and_then(Value::as_str) != Some("FAULT")
            || fields.get("default_disposition").and_then(Value::as_str) != Some("observe_only")
        {
            return None;
        }
        let transaction_hash = record.transaction_hash.clone()?;
        let execution_stage = fields.get("execution_stage")?.as_str()?;
        let exception_category = fields.get("exception_category")?.as_str()?;
        let gas_consumed = fields.get("gas_consumed")?.as_i64()?;
        let schema_version = fields.get("schema_version")?.as_u64()?;
        let observation_rule_version = fields.get("observation_rule_version")?.as_str()?;
        if execution_stage != "application"
            || !matches!(exception_category, "vm_fault" | "caught_vm_exception")
            || gas_consumed < 0
            || schema_version != u64::from(TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION)
            || observation_rule_version != TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION
        {
            return None;
        }
        let signature = format!(
            "{execution_stage}|{exception_category}|{gas_consumed}|{schema_version}|{observation_rule_version}"
        );
        let evidence_ids = {
            let mut repetitions = self.inner.execution_repetitions.lock();
            if !repetitions.contains_key(&transaction_hash)
                && repetitions.len() >= self.inner.config.evidence_queue_capacity.max(1)
                && let Some(first_key) = repetitions.keys().next().cloned()
            {
                repetitions.remove(&first_key);
            }
            let entry = repetitions
                .entry(transaction_hash.clone())
                .or_insert_with(|| ExecutionRepetition {
                    signature: signature.clone(),
                    evidence_ids: Vec::with_capacity(2),
                    candidate_emitted: false,
                });
            if entry.signature != signature {
                entry.signature = signature;
                entry.evidence_ids.clear();
                entry.candidate_emitted = false;
            }
            if !entry.evidence_ids.iter().any(|id| id == &record.id) {
                entry.evidence_ids.push(record.id.clone());
            }
            if entry.evidence_ids.len() < 2 || entry.candidate_emitted {
                return None;
            }
            entry.candidate_emitted = true;
            entry.evidence_ids.clone()
        };
        let finding = EvidenceItem::new(
            format!("{}:execution-reproducible", record.id),
            EvidenceKind::DetectorFinding,
            EvidenceSource::DeterministicDetector,
            record.observed_height,
            BTreeMap::from([
                ("transaction_hash".to_string(), json!(transaction_hash)),
                ("parent_evidence_ids".to_string(), json!(evidence_ids)),
                ("reproducible_count".to_string(), json!(2)),
                (
                    "candidate_action".to_string(),
                    json!("restricted_probe_candidate"),
                ),
                (
                    "rule_version".to_string(),
                    json!(self.inner.config.rule_version),
                ),
            ]),
        )
        .ok()?;
        EvidenceRecord::new_with_context(
            finding,
            EvidenceAuthority::DeterministicRule,
            record.observed_at_unix_ms,
            Some(transaction_hash),
            record.tip_height,
            record.tip_hash.clone(),
            self.inner.config.rule_version.clone(),
        )
        .ok()
    }

    /// Fixed deterministic transaction-validation rule.
    ///
    /// A trusted local observation of a deterministic mempool validation
    /// outcome produces a transaction-bound `DeterministicRule` finding and
    /// drives only the bounded local mempool quarantine (or its release).
    /// This is the mempool's automatic-quarantine contract: the finding is
    /// constructed exclusively by this fixed local detector from
    /// Observation-authority records, and model-authority records terminate
    /// before it in the worker, so they can never reach this effect path.
    pub(super) async fn handle_transaction_validation(&self, record: &EvidenceRecord) {
        let Some(transaction_hash) = record.transaction_hash.clone() else {
            return;
        };
        let fields = record.item.fields();
        if fields.get("deterministic").and_then(Value::as_bool) != Some(true) {
            return;
        }
        let Some(validation_result) = fields
            .get("validation_result")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return;
        };
        // Plan §43 evidence threshold: a single deterministic failure only
        // records the finding. Quarantine escalates once the same transaction
        // accumulates repeated deterministic failures under the same rule
        // version, or the failure reproduces across block heights. A trusted
        // succeed observation always releases and clears the evidence.
        let (apply, quarantine) = if validation_result == "succeed" {
            self.inner
                .validation_failures
                .lock()
                .remove(&transaction_hash);
            (true, false)
        } else {
            let escalate =
                self.record_validation_failure(&transaction_hash, record.observed_height);
            (escalate, true)
        };
        let finding_fields = BTreeMap::from([
            ("parent_evidence_id".to_string(), json!(record.id)),
            ("transaction_hash".to_string(), json!(transaction_hash)),
            ("validation_result".to_string(), json!(validation_result)),
            (
                "rule_version".to_string(),
                json!(self.inner.config.rule_version),
            ),
            (
                "action".to_string(),
                json!(if validation_result == "succeed" {
                    "release_transaction"
                } else if apply {
                    "quarantine_transaction"
                } else {
                    "record_only"
                }),
            ),
        ]);
        let Ok(item) = EvidenceItem::new(
            format!("{}:transaction-validation", record.id),
            EvidenceKind::DetectorFinding,
            EvidenceSource::DeterministicDetector,
            record.observed_height,
            finding_fields,
        ) else {
            return;
        };
        let Ok(finding) = EvidenceRecord::new_with_context(
            item,
            EvidenceAuthority::DeterministicRule,
            record.observed_at_unix_ms,
            Some(transaction_hash.clone()),
            record.tip_height,
            record.tip_hash.clone(),
            self.inner.config.rule_version.clone(),
        ) else {
            return;
        };
        self.inner.registry.lock().insert(finding.clone());
        if apply {
            self.apply_transaction_validation_finding(&finding, quarantine, &transaction_hash)
                .await;
        }
    }

    /// Records one deterministic validation failure and reports whether the
    /// §43 quarantine evidence threshold has been reached.
    fn record_validation_failure(&self, transaction_hash: &str, observed_height: u32) -> bool {
        let mut failures = self.inner.validation_failures.lock();
        if !failures.contains_key(transaction_hash)
            && failures.len() >= self.inner.config.evidence_queue_capacity.max(1)
            && let Some(first_key) = failures.keys().next().cloned()
        {
            failures.remove(&first_key);
        }
        let entry = failures
            .entry(transaction_hash.to_owned())
            .or_insert_with(|| ValidationFailure {
                rule_version: self.inner.config.rule_version.clone(),
                deterministic_failures: 0,
                heights: BTreeSet::new(),
            });
        if entry.rule_version != self.inner.config.rule_version {
            // A rule-version change invalidates previously accumulated
            // evidence: the failure counter restarts under the new rules.
            entry
                .rule_version
                .clone_from(&self.inner.config.rule_version);
            entry.deterministic_failures = 0;
            entry.heights.clear();
        }
        entry.deterministic_failures = entry.deterministic_failures.saturating_add(1);
        entry.heights.insert(observed_height);
        entry.deterministic_failures >= QUARANTINE_EVIDENCE_FAILURES
            || entry.heights.len() >= QUARANTINE_EVIDENCE_FAILURES as usize
    }

    /// Applies the fixed transaction-validation finding through the bounded
    /// local mempool quarantine contract and audits the durable binding.
    ///
    /// Mirrors the audited effect protocol of
    /// [`AiAgentRuntime::apply_action`]: an "authorized" audit entry must be
    /// durably appended before any effect is allowed (an append failure
    /// rejects the effect entirely), and the executed effect must be audited
    /// afterwards; if that final append fails, a `reconciliation_required`
    /// audit record is appended and the action result reflects the broken
    /// audit trail instead of a clean outcome. The mempool enforces its own
    /// enablement (automatic quarantine is configured from
    /// `allow_transaction_quarantine`), capacity, impact and lifetime
    /// ceilings, so a failed or disabled effect is audited as a non-executed
    /// evaluation without mutating quarantine state.
    async fn apply_transaction_validation_finding(
        &self,
        finding: &EvidenceRecord,
        quarantine: bool,
        transaction_hash: &str,
    ) -> ActionResult {
        let request = action_request(
            if quarantine {
                OperationalAction::QuarantineTransaction
            } else {
                OperationalAction::ReleaseTransaction
            },
            Some(transaction_hash.to_owned()),
            finding,
        );
        // Authorization must be durably audited before any effect is allowed.
        if self
            .audit_action(&request, std::slice::from_ref(finding), "authorized", false)
            .await
            .is_err()
        {
            return ActionResult::Failed;
        }
        let executed = if quarantine {
            self.quarantine_from_local_finding(finding, transaction_hash)
        } else {
            self.release_from_local_finding(transaction_hash)
        };
        let outcome = if executed { "executed" } else { "failed" };
        if self
            .audit_action(&request, std::slice::from_ref(finding), outcome, executed)
            .await
            .is_err()
        {
            if executed {
                // The effect landed but its executed-audit entry is not
                // durable: record the reconciliation requirement before
                // surfacing the non-clean result.
                self.audit_reconciliation_required(&request, std::slice::from_ref(finding))
                    .await;
            }
            return ActionResult::Failed;
        }
        if executed {
            ActionResult::Executed
        } else {
            ActionResult::Failed
        }
    }

    /// Appends the durable record that an executed local effect lacks its
    /// executed-audit entry and therefore requires reconciliation.
    async fn audit_reconciliation_required(
        &self,
        request: &OperationalActionRequest,
        records: &[EvidenceRecord],
    ) {
        let reconciled = self
            .inner
            .audit
            .append(AuditInput {
                timestamp_unix_ms: unix_time_ms(),
                kind: AuditEventKind::OperationalActionEvaluated,
                observed_height: self.inner.host.chain.current_block_index(),
                evidence_refs: records.iter().map(|record| record.id.clone()).collect(),
                evidence_hashes: records
                    .iter()
                    .map(|record| record.content_hash.clone())
                    .collect(),
                action: Some(format!("{:?}", request.action)),
                target_hash: request.target.clone(),
                rule_version: records
                    .first()
                    .map(|record| record.rule_version.clone())
                    .or_else(|| Some(self.inner.config.rule_version.clone())),
                outcome_code: "reconciliation_required".to_owned(),
            })
            .await
            .is_ok();
        if !reconciled {
            // Even the reconciliation record is undurable; the failed action
            // result already surfaces the broken audit trail, and the append
            // failure is counted instead of silently dropped.
            self.inner
                .audit_append_failures
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    fn quarantine_from_local_finding(&self, finding: &EvidenceRecord, target: &str) -> bool {
        let Some(transaction_hash) = self.resolve_target_hash(target) else {
            return false;
        };
        let now = unix_time_ms();
        if self
            .inner
            .host
            .mempool
            .is_quarantined(&transaction_hash, now)
        {
            // The transaction is already actively contained: repeated
            // deterministic findings stay idempotent instead of failing
            // with the mempool's AlreadyActive error.
            return true;
        }
        let metadata = QuarantineMetadata {
            transaction_hash,
            evidence_refs: vec![finding.id.clone()],
            context: "deterministic transaction validation".to_string(),
            created_at_unix_ms: now,
            expires_at_unix_ms: now.saturating_add(self.inner.config.quarantine_ttl_ms),
            rule_version: self.inner.config.rule_version.clone(),
            episode: 0,
            cumulative_active_ms: 0,
        };
        self.inner.host.mempool.quarantine(metadata, now).is_ok()
    }

    fn release_from_local_finding(&self, target: &str) -> bool {
        self.resolve_target_hash(target)
            .is_some_and(|transaction_hash| self.inner.host.mempool.release(&transaction_hash))
    }
}
