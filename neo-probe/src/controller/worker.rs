use super::runner::RunCancellation;
use super::*;
pub(super) fn validate_request_envelope(request: &ProbeRequestFrame) -> Result<(), ()> {
    request.job.validate().map_err(|_| ())?;
    let expected = ProbeBindings::calculate(&request.job).map_err(|_| ())?;
    if request.bindings != expected
        || request.snapshot.root_hash != request.job.context.tip_hash
        || request.snapshot.height != request.job.context.observed_height
    {
        return Err(());
    }
    encode_frame(request).map_err(|_| ())?;
    Ok(())
}
pub(super) async fn worker_loop(
    mut receiver: mpsc::Receiver<WorkItem>,
    config: ProbeControllerConfig,
    action_log: Arc<DurableActionStore>,
    launcher: Arc<dyn ProcessLauncher>,
    context_rechecker: Arc<dyn ProbeContextRechecker>,
    completions: mpsc::Sender<ProbeControllerResult>,
    shutdown: CancellationToken,
) {
    loop {
        let item = tokio::select! {
            biased;
            () = shutdown.cancelled() => {
                receiver.close();
                while let Ok(item) = receiver.try_recv() {
                    close_queued_item(&action_log, item);
                }
                break;
            }
            item = receiver.recv() => match item {
                Some(item) => item,
                None => break,
            },
        };
        let result = execute_item(
            &config,
            &action_log,
            launcher.as_ref(),
            context_rechecker.as_ref(),
            &shutdown,
            &item,
        )
        .await;
        // Bounded, non-blocking completion fan-out for the runtime consumer:
        // a full completion queue drops the notification instead of blocking
        // the single controller worker here.
        let _ = completions.try_send(result.clone());
        let _ = item.result.send(result);
    }
}

fn close_queued_item(action_log: &DurableActionStore, item: WorkItem) {
    let job = &item.request.job;
    if begin_action(action_log, job).is_ok() {
        mark_uncertain(action_log, &job.operation_id);
    }
    let _ = item.result.send(ProbeControllerResult {
        operation_id: job.operation_id.clone(),
        run_id: job.run_id.clone(),
        verdict: ProbeVerdict::Uncertain(ProbeUncertainReason::ControllerClosed),
    });
}

async fn execute_item(
    config: &ProbeControllerConfig,
    action_log: &DurableActionStore,
    launcher: &dyn ProcessLauncher,
    context_rechecker: &dyn ProbeContextRechecker,
    shutdown: &CancellationToken,
    item: &WorkItem,
) -> ProbeControllerResult {
    let job = &item.request.job;
    let uncertain = |reason| ProbeControllerResult {
        operation_id: job.operation_id.clone(),
        run_id: job.run_id.clone(),
        verdict: ProbeVerdict::Uncertain(reason),
    };

    if !context_rechecker.is_current(job) {
        return uncertain(ProbeUncertainReason::ContextChanged);
    }
    if begin_action(action_log, job).is_err() {
        return uncertain(ProbeUncertainReason::ActionStore);
    }

    let Ok(framed) = encode_frame(&item.request) else {
        mark_uncertain(action_log, &job.operation_id);
        return uncertain(ProbeUncertainReason::Protocol);
    };
    let suite_deadline = Instant::now() + config.suite_timeout;
    let cancellation = RunCancellation {
        controller: shutdown.clone(),
        job: item.cancellation.clone(),
    };
    let mut outcomes = Vec::with_capacity(2);
    for _ in 0..2 {
        let output = match launcher
            .run(
                &config.executable,
                framed.clone(),
                config.replay_timeout,
                suite_deadline,
                config.memory_limit_bytes,
                config.frame_limit_bytes,
                cancellation.clone(),
            )
            .await
        {
            Ok(output) => output,
            Err(error) => {
                mark_uncertain(action_log, &job.operation_id);
                return uncertain(error.reason());
            }
        };
        if oom_indicator(&output.stderr) {
            mark_uncertain(action_log, &job.operation_id);
            return uncertain(ProbeUncertainReason::OutOfMemory);
        }
        if !output.success {
            mark_uncertain(action_log, &job.operation_id);
            return uncertain(ProbeUncertainReason::ProcessFailed);
        }
        let Ok(outcome) = decode_frame::<ProbeOutcome>(&output.stdout) else {
            mark_uncertain(action_log, &job.operation_id);
            return uncertain(ProbeUncertainReason::Protocol);
        };
        if outcome.status != ProbeOutcomeStatus::Succeeded {
            mark_uncertain(action_log, &job.operation_id);
            return uncertain(ProbeUncertainReason::WorkerRejected);
        }
        outcomes.push(outcome);
    }

    if outcomes[0] != outcomes[1] {
        mark_uncertain(action_log, &job.operation_id);
        return uncertain(ProbeUncertainReason::ReplayMismatch);
    }
    let Ok(identity) = TrustedLocalDiagnosticsIdentity::from_job(job) else {
        mark_uncertain(action_log, &job.operation_id);
        return uncertain(ProbeUncertainReason::BindingMismatch);
    };
    let Ok(validated) = ProbeResultValidator.validate_double_replay(
        &identity,
        job,
        outcomes.remove(0),
        outcomes.remove(0),
        unix_time_ms(),
    ) else {
        mark_uncertain(action_log, &job.operation_id);
        return uncertain(ProbeUncertainReason::BindingMismatch);
    };
    if complete_action(action_log, &job.operation_id).is_err() {
        return uncertain(ProbeUncertainReason::ActionStore);
    }
    ProbeControllerResult {
        operation_id: job.operation_id.clone(),
        run_id: job.run_id.clone(),
        verdict: ProbeVerdict::Completed(validated),
    }
}
fn begin_action(action_log: &DurableActionStore, job: &ProbeJob) -> Result<(), ()> {
    let binding_hash = job.binding_hash().map_err(|_| ())?;
    let binding = ActionBinding::new(
        job.operation_id.clone(),
        OperationalAction::StartProbeRequest,
        Some(format!("{}:{binding_hash}", job.run_id)),
        vec![job.authorizer.evidence_id.clone()],
        vec![job.authorizer.evidence_hash.clone()],
        job.authorizer.rule_version.clone(),
    )
    .map_err(|_| ())?;
    match action_log.admit(binding, unix_time_ms()).map_err(|_| ())? {
        AdmitOutcome::Admitted(_) => {}
        AdmitOutcome::Existing(_) => return Err(()),
    }
    action_log
        .mark_running(&job.operation_id, unix_time_ms())
        .map_err(|_| ())?;
    Ok(())
}

fn complete_action(action_log: &DurableActionStore, operation_id: &OperationId) -> Result<(), ()> {
    if action_log
        .mark_completed(operation_id, unix_time_ms())
        .is_err()
    {
        mark_uncertain(action_log, operation_id);
        return Err(());
    }
    Ok(())
}

fn mark_uncertain(action_log: &DurableActionStore, operation_id: &OperationId) {
    let _ = action_log.mark_uncertain(operation_id, unix_time_ms());
}

fn oom_indicator(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    [
        "out of memory",
        "cannot allocate memory",
        "memory allocation failed",
        "oom",
        "status_no_memory",
    ]
    .iter()
    .any(|indicator| stderr.contains(indicator))
}
