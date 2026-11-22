use super::*;

use super::worker::SharedModelProvider;

impl AiAgentRuntime {
    #[doc(hidden)]
    pub fn request_worker_restart(&self) {
        if self.inner.config.enabled {
            self.inner.worker_generation.fetch_add(1, Ordering::AcqRel);
            self.inner.worker_restart.notify_one();
        }
    }

    pub(super) fn start_worker_supervisor(
        &self,
        mut receiver: EvidenceReceiver,
        mut execution_receiver: mpsc::Receiver<ExecutionObservation>,
        mut probe_completions: Option<mpsc::Receiver<ProbeCompletion>>,
        model_provider: Arc<NodeModelProvider>,
    ) -> anyhow::Result<()> {
        let audit = Arc::clone(&self.inner.audit);
        let restart = Arc::clone(&self.inner.worker_restart);
        let weak: Weak<RuntimeInner> = Arc::downgrade(&self.inner);
        let task = tokio::spawn(async move {
            let Ok(detector) = StallDetector::new(StallPolicy::default()) else {
                return;
            };
            let provider = SharedModelProvider(model_provider);
            let mut agent = DiagnosticAgent::new(
                detector,
                provider.clone(),
                Arc::clone(&audit),
                DiagnosisValidator::new(2),
                512,
            );
            let mut prefer_evidence = true;
            loop {
                let input = match receive_worker_event(
                    &mut receiver,
                    &mut execution_receiver,
                    &mut probe_completions,
                    &restart,
                    prefer_evidence,
                )
                .await
                {
                    WorkerEvent::Restart => {
                        let Ok(detector) = StallDetector::new(StallPolicy::default()) else {
                            return;
                        };
                        agent = DiagnosticAgent::new(
                            detector,
                            provider.clone(),
                            Arc::clone(&audit),
                            DiagnosisValidator::new(2),
                            512,
                        );
                        continue;
                    }
                    WorkerEvent::ProbeCompletionsClosed => {
                        // The controller is gone; retire this arm instead of
                        // busy-polling a closed receiver.
                        probe_completions = None;
                        continue;
                    }
                    WorkerEvent::Input(input) => input,
                    WorkerEvent::Closed => return,
                };
                prefer_evidence = matches!(&input, WorkerInput::Execution(_));
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let runtime = AiAgentRuntime { inner };
                let record = match input {
                    WorkerInput::Evidence(record) => record,
                    WorkerInput::ProbeCompletion(completion) => {
                        // Real production consumption path: the controller's
                        // bounded non-blocking completion stream reaches the
                        // trusted result policy here, not only from tests.
                        let _ = runtime.apply_probe_completion(completion).await;
                        continue;
                    }
                    WorkerInput::Execution(observation) => {
                        let Some(record) = runtime.execution_observation_record(&observation)
                        else {
                            continue;
                        };
                        record
                    }
                };
                runtime
                    .inner
                    .received_events
                    .fetch_add(1, Ordering::Relaxed);
                if runtime.validate_record(&record, false).is_err() {
                    continue;
                }

                let item = record.item.clone();
                let observed_at = record.observed_at_unix_ms;
                runtime.inner.registry.lock().insert(record.clone());

                // Model output remains available as advisory evidence, but it must
                // terminate before any local state update, fixed detector, or action chain.
                if record.authority == EvidenceAuthority::Model {
                    if runtime
                        .audit_detector_rejection(&record, "model_authority_rejected")
                        .await
                        .is_err()
                    {
                        // The rejection itself stands (model evidence never
                        // proceeds), but the undurable audit append is counted
                        // instead of silently dropped.
                        runtime
                            .inner
                            .audit_append_failures
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    continue;
                }
                if let Some(candidate) = runtime.reproducible_execution_candidate(&record) {
                    runtime.inner.registry.lock().insert(candidate.clone());
                    // Plan §38 closed loop: the candidate finding is consumed
                    // through the controlled probe submission path, not merely
                    // recorded.
                    runtime.handle_restricted_probe_candidate(&candidate);
                }
                // Fixed deterministic transaction-validation detector: only
                // trusted local Observation-authority records reach it (model
                // records terminated above), and it alone constructs the
                // transaction-bound deterministic finding for the bounded
                // local mempool quarantine contract.
                if item.kind == EvidenceKind::TransactionValidation
                    && record.authority == EvidenceAuthority::Observation
                    && item.source == EvidenceSource::NodeRuntime
                {
                    runtime.handle_transaction_validation(&record).await;
                }
                if item.kind == EvidenceKind::ChainCommit
                    && record.authority == EvidenceAuthority::Observation
                    && item.source == EvidenceSource::NodeRuntime
                {
                    advance_last_progress(&runtime.inner.last_progress_unix_ms, observed_at);
                }

                if item.kind == EvidenceKind::ConsensusState
                    && record.authority == EvidenceAuthority::Observation
                    && item.source == EvidenceSource::ConsensusSnapshot
                    && let Some(observation) = consensus_observation(&item, observed_at)
                    && let Ok(bundle) =
                        EvidenceBundle::new(observed_at, item.observed_height, vec![item.clone()])
                {
                    runtime
                        .inner
                        .analyzed_events
                        .fetch_add(1, Ordering::Relaxed);
                    if let Ok(report) = agent.analyze(&observation, &bundle).await
                        && report.detection.stall_level().is_some()
                    {
                        let finding_id = format!("{}:stall-rule", item.id);
                        let fields = BTreeMap::from([
                            ("parent_evidence_id".to_string(), json!(item.id)),
                            (
                                "detection".to_string(),
                                json!(format!("{:?}", report.detection)),
                            ),
                            (
                                "rule_version".to_string(),
                                json!(runtime.inner.config.rule_version),
                            ),
                            ("action".to_string(), json!("increase_sampling")),
                        ]);
                        if let Ok(finding) = EvidenceItem::new(
                            finding_id,
                            EvidenceKind::DetectorFinding,
                            EvidenceSource::DeterministicDetector,
                            item.observed_height,
                            fields,
                        ) && let Ok(finding_record) = EvidenceRecord::new_with_context(
                            finding,
                            EvidenceAuthority::DeterministicRule,
                            observed_at,
                            None,
                            record.tip_height,
                            record.tip_hash.clone(),
                            runtime.inner.config.rule_version.clone(),
                        ) {
                            runtime.inner.registry.lock().insert(finding_record.clone());
                            let request = action_request(
                                OperationalAction::IncreaseSampling,
                                None,
                                &finding_record,
                            );
                            let _ = runtime.apply_action(request).await;
                        }
                    }
                }
            }
        });
        self.inner.worker.lock().task = Some(task);
        Ok(())
    }

    async fn audit_detector_rejection(
        &self,
        record: &EvidenceRecord,
        outcome_code: &str,
    ) -> Result<(), crate::AuditError> {
        self.inner
            .audit
            .append(AuditInput {
                timestamp_unix_ms: unix_time_ms(),
                kind: AuditEventKind::DiagnosisRejected,
                observed_height: record.observed_height,
                evidence_refs: vec![record.id.clone()],
                evidence_hashes: vec![record.content_hash.clone()],
                action: None,
                target_hash: record.transaction_hash.clone(),
                rule_version: Some(self.inner.config.rule_version.clone()),
                outcome_code: outcome_code.to_owned(),
            })
            .await
            .map(|_| ())
    }

    pub(super) fn is_quarantined_target(&self, request: &OperationalActionRequest) -> bool {
        request
            .target
            .as_deref()
            .and_then(|target| self.resolve_target_hash(target))
            .is_some_and(|hash| {
                self.inner
                    .host
                    .mempool
                    .is_quarantined(&hash, unix_time_ms())
            })
    }

    pub(super) fn start_sampler(&self) {
        let weak: Weak<RuntimeInner> = Arc::downgrade(&self.inner);
        let task = tokio::spawn(async move {
            loop {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let interval = inner.sampling_interval_ms.load(Ordering::Acquire).max(1);
                tokio::select! {
                    () = tokio::time::sleep(Duration::from_millis(interval)) => {
                        inner.sampling_ticks.fetch_add(1, Ordering::Relaxed);
                        let runtime = AiAgentRuntime { inner: Arc::clone(&inner) };
                        let observed_at = unix_time_ms();
                        let height = inner.host.chain.current_block_index();
                        let previous_height = inner
                            .last_observed_local_height
                            .swap(u64::from(height), Ordering::AcqRel);
                        if u64::from(height) > previous_height {
                            advance_last_progress(&inner.last_progress_unix_ms, observed_at);
                        }
                        if let Some(peers) = inner.host.chain.peer_observation().await {
                            inner
                                .observed_peer_count
                                .store(peers.connected_peers, Ordering::Release);
                            inner
                                .observed_peer_height
                                .store(peers.max_peer_height, Ordering::Release);
                            inner
                                .peer_observed_at_unix_ms
                                .store(observed_at, Ordering::Release);
                            inner
                                .peer_observation_available
                                .store(true, Ordering::Release);
                        }
                        let mut fields = BTreeMap::from([
                            ("local_height".to_string(), json!(height)),
                            ("sample_interval_ms".to_string(), json!(interval)),
                            (
                                "last_progress_unix_ms".to_string(),
                                json!(inner.last_progress_unix_ms.load(Ordering::Acquire)),
                            ),
                        ]);
                        if inner.peer_observation_available.load(Ordering::Acquire) {
                            fields.insert(
                                "connected_peers".to_string(),
                                json!(inner.observed_peer_count.load(Ordering::Acquire)),
                            );
                            fields.insert(
                                "max_peer_height".to_string(),
                                json!(inner.observed_peer_height.load(Ordering::Acquire)),
                            );
                            fields.insert(
                                "peer_observed_at_unix_ms".to_string(),
                                json!(inner.peer_observed_at_unix_ms.load(Ordering::Acquire)),
                            );
                        }
                        let _ = runtime.try_export(
                            EvidenceKind::ChainProgress,
                            EvidenceSource::NodeMetrics,
                            height,
                            fields,
                        );
                    }
                    () = inner.sampling_changed.notified() => {}
                }
            }
        });
        self.inner.worker.lock().sampler = Some(task);
    }

    pub(super) async fn audit_action(
        &self,
        request: &OperationalActionRequest,
        records: &[EvidenceRecord],
        outcome: &str,
        executed: bool,
    ) -> Result<(), crate::AuditError> {
        self.inner
            .audit
            .append(AuditInput {
                timestamp_unix_ms: unix_time_ms(),
                kind: if executed {
                    AuditEventKind::OperationalActionExecuted
                } else {
                    AuditEventKind::OperationalActionEvaluated
                },
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
                outcome_code: outcome.to_string(),
            })
            .await
            .map(|_| ())
    }

    #[doc(hidden)]
    pub async fn audit_snapshot(&self) -> Vec<crate::AuditEvent> {
        self.inner.audit.snapshot().await.unwrap_or_default()
    }

    #[doc(hidden)]
    pub fn transaction_finding(&self, hash: &str) -> Option<EvidenceRecord> {
        self.inner
            .registry
            .lock()
            .records
            .values()
            .find(|record| {
                record.authority == EvidenceAuthority::DeterministicRule
                    && record.transaction_hash.as_deref() == Some(hash)
            })
            .cloned()
    }

    #[doc(hidden)]
    pub fn execution_observation(&self, hash: &str) -> Option<EvidenceRecord> {
        self.inner
            .registry
            .lock()
            .records
            .values()
            .find(|record| {
                record.authority == EvidenceAuthority::Observation
                    && record.kind == EvidenceKind::TransactionExecution
                    && record.transaction_hash.as_deref() == Some(hash)
            })
            .cloned()
    }
}
