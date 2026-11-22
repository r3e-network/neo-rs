use super::*;
pub(super) fn validate_identity(identity: Value, settings: &ProtocolSettings) -> Result<()> {
    // Public seed operators may choose a different local block-production interval.
    // Execution-affecting MainNet parameters and hardfork heights must still match.
    let mut expected = settings.clone();
    if let Some(milliseconds) = identity
        .get("protocol")
        .and_then(|protocol| protocol.get("msperblock"))
        .and_then(Value::as_u64)
    {
        expected.milliseconds_per_block = u32::try_from(milliseconds)
            .context("upstream msperblock exceeds the supported range")?;
    }
    validate_protocol_identity(&identity, &expected)
}

struct EvidenceBatch {
    primary: Vec<UpstreamEvidence>,
    secondary: Vec<UpstreamEvidence>,
    fetch_seconds: f64,
}

async fn fetch_batch(
    primary: &RpcEndpoint,
    secondary: &RpcEndpoint,
    start: u32,
    end: u32,
    verify_receipts: bool,
) -> Result<EvidenceBatch> {
    let started = Instant::now();
    let (primary, secondary) = tokio::join!(
        primary.evidence(start, end, verify_receipts),
        secondary.evidence(start, end, verify_receipts),
    );
    let primary = primary?;
    let secondary = secondary?;
    if primary.len() != secondary.len() {
        bail!("upstream evidence batch lengths differ");
    }
    Ok(EvidenceBatch {
        primary,
        secondary,
        fetch_seconds: started.elapsed().as_secs_f64(),
    })
}

fn spawn_fetch(
    primary: &RpcEndpoint,
    secondary: &RpcEndpoint,
    start: u32,
    end: u32,
    verify_receipts: bool,
) -> tokio::task::JoinHandle<Result<EvidenceBatch>> {
    let primary = primary.clone();
    let secondary = secondary.clone();
    tokio::spawn(
        async move { fetch_batch(&primary, &secondary, start, end, verify_receipts).await },
    )
}

pub(super) async fn verify_range(
    executor: &NeoExecutor,
    primary: &RpcEndpoint,
    secondary: &RpcEndpoint,
    start: u32,
    target: u32,
    batch_size: u32,
    verify_receipts: bool,
    prefetch: bool,
    batch_durable: bool,
    mismatch_path: &Path,
    audit: &mut impl Write,
    executed: &mut u64,
    receipts: &mut u64,
    profile: &mut ReplayProfile,
    process_memory: &mut Option<ProcessMemoryMonitor>,
) -> Result<()> {
    if start > target {
        return Ok(());
    }
    let started = Instant::now();
    let mut batch_start = start;
    let first_end = batch_start.saturating_add(batch_size - 1).min(target);
    let mut pending =
        prefetch.then(|| spawn_fetch(primary, secondary, batch_start, first_end, verify_receipts));
    while batch_start <= target {
        let batch_end = batch_start.saturating_add(batch_size - 1).min(target);
        let stage = Instant::now();
        let batch = match pending.take() {
            Some(task) => task.await.context("upstream fetch task failed")??,
            None => {
                fetch_batch(primary, secondary, batch_start, batch_end, verify_receipts).await?
            }
        };
        profile.upstream_wait_seconds += stage.elapsed().as_secs_f64();
        profile.upstream_seconds += batch.fetch_seconds;
        let mut next_pending = if prefetch && batch_end < target {
            let next_start = batch_end + 1;
            let next_end = next_start.saturating_add(batch_size - 1).min(target);
            Some(spawn_fetch(
                primary,
                secondary,
                next_start,
                next_end,
                verify_receipts,
            ))
        } else {
            None
        };
        let result = process_batch(
            executor,
            batch.primary,
            batch.secondary,
            verify_receipts,
            !batch_durable,
            mismatch_path,
            audit,
            executed,
            receipts,
            profile,
        )
        .and_then(|()| {
            if batch_durable {
                let stage = Instant::now();
                let flushed = executor.flush_durable();
                profile.durable_flush_seconds += stage.elapsed().as_secs_f64();
                flushed?;
            }
            let stage = Instant::now();
            let flushed = audit.flush();
            profile.audit_seconds += stage.elapsed().as_secs_f64();
            flushed.map_err(Into::into)
        });
        if let Err(error) = result {
            if let Some(task) = next_pending.take() {
                task.abort();
                let _ = task.await;
            }
            return Err(error);
        }
        pending = next_pending;
        if let Some(monitor) = process_memory.as_mut() {
            monitor.sample()?;
        }
        let elapsed = started.elapsed().as_secs_f64();
        println!(
            "verified={batch_end} target={target} batch={batch_start}-{batch_end} executed={} receipts={} rate={:.2} blocks/s upstream={:.2}s wait={:.2}s execute={:.2}s audit={:.2}s",
            *executed,
            *receipts,
            *executed as f64 / elapsed.max(f64::EPSILON),
            profile.upstream_seconds,
            profile.upstream_wait_seconds,
            profile.local_execution_seconds,
            profile.audit_seconds,
        );
        batch_start = batch_end.checked_add(1).context("block height overflow")?;
    }
    Ok(())
}

fn process_batch(
    executor: &NeoExecutor,
    primary: Vec<UpstreamEvidence>,
    secondary: Vec<UpstreamEvidence>,
    verify_receipts: bool,
    flush_each_block: bool,
    mismatch_path: &Path,
    audit: &mut impl Write,
    executed: &mut u64,
    receipts: &mut u64,
    profile: &mut ReplayProfile,
) -> Result<()> {
    for (primary, secondary) in primary.into_iter().zip(secondary) {
        let height = primary.height;
        let stage = Instant::now();
        let evidence = compare_evidence(primary, secondary)?;
        let block = decode_block(&evidence.raw_block, height)?;
        let block_hash = block.hash();
        let transaction_count = block.transactions.len();
        profile.decode_compare_seconds += stage.elapsed().as_secs_f64();
        let stage = Instant::now();
        let outcome = executor
            .execute(block)
            .with_context(|| format!("neo-rs failed to execute block {height}"))?;
        profile.local_execution_seconds += stage.elapsed().as_secs_f64();
        if outcome.block_hash != block_hash {
            bail!("local block hash changed during execution at {height}");
        }
        if outcome.state_root != evidence.state_root {
            let incident = json!({
                "height": height,
                "code": "state_root_mismatch",
                "block_hash": block_hash.to_string(),
                "local_state_root": outcome.state_root.to_string(),
                "upstream_state_root": evidence.state_root.to_string(),
            });
            write_mismatch(mismatch_path, audit, &incident)?;
            bail!("state-root mismatch at {height}: {incident}");
        }
        let stage = Instant::now();
        let receipt_count = if verify_receipts {
            match compare_execution_receipts(
                height,
                evidence.executions.as_deref(),
                outcome.executions.as_deref(),
            ) {
                Ok(count) => count,
                Err(error) => {
                    let incident = json!({
                        "height": height,
                        "code": "execution_receipt_mismatch",
                        "block_hash": block_hash.to_string(),
                        "detail": format!("{error:#}"),
                    });
                    write_mismatch(mismatch_path, audit, &incident)?;
                    return Err(error);
                }
            }
        } else {
            0
        };
        profile.receipt_compare_seconds += stage.elapsed().as_secs_f64();
        let stage = Instant::now();
        serde_json::to_writer(
            &mut *audit,
            &AuditRecord {
                height,
                block_hash: block_hash.to_string(),
                state_root: outcome.state_root.to_string(),
                transaction_count,
                receipts_verified: verify_receipts,
                receipt_count,
                offline_block_offset: None,
                offline_next_offset: None,
            },
        )?;
        audit.write_all(b"\n")?;
        // Default mode persists each audit watermark immediately. Batch-durable
        // mode flushes only after the enclosing batch's WAL sync succeeds.
        if flush_each_block {
            audit.flush()?;
        }
        profile.audit_seconds += stage.elapsed().as_secs_f64();
        *executed += 1;
        *receipts = receipts
            .checked_add(u64::try_from(receipt_count)?)
            .context("execution receipt counter overflow")?;
    }
    Ok(())
}
pub(super) fn validate_audit_resume_mode(
    file: &mut File,
    checkpoint: Option<&neo_validator::state::VerifiedPoint>,
    verify_receipts: bool,
    new_audit_at_checkpoint: bool,
) -> Result<()> {
    if !new_audit_at_checkpoint {
        return validate_audit_resume(file, checkpoint, verify_receipts);
    }
    if checkpoint.is_none() {
        bail!("--new-audit-at-checkpoint requires an existing validator checkpoint");
    }
    if file.metadata()?.len() != 0 {
        bail!("--new-audit-at-checkpoint requires a missing or empty audit file");
    }
    Ok(())
}
fn write_mismatch(path: &Path, audit: &mut impl Write, incident: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(incident)?)?;
    audit.flush()?;
    Ok(())
}
