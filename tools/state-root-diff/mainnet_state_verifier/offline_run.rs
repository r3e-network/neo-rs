use super::config::ExecutionModeArg;
use super::offline::{OfflineBlock, OfflineBlockSource, read_last_audit_record};
use super::profile::{ExecutionStageProfile, MptNodeCacheProfile, ReplayProfile};
use super::*;
#[derive(Debug, Serialize)]
struct OfflineRunSummary {
    network: String,
    source: String,
    package_start: u32,
    package_end: u32,
    execution_mode: String,
    local_replay_only: bool,
    receipts_verified: bool,
    batch_durable: bool,
    empty_block_write_back: bool,
    mpt_node_cache: MptNodeCacheProfile,
    process_memory: Option<ProcessMemoryProfile>,
    execution_profile: Option<ExecutionStageProfile>,
    initial_height: Option<u32>,
    verified_through: Option<u32>,
    target_height: u32,
    blocks_executed_this_run: u64,
    direct_transfers_this_run: u64,
    elapsed_seconds: f64,
    replay_profile: ReplayProfile,
    complete_for_target: bool,
}

pub(super) async fn run_offline(args: Args, package: &Path) -> Result<()> {
    if !matches!(args.execution_mode, ExecutionModeArg::Canonical) {
        bail!("offline full-chain replay requires --execution-mode canonical");
    }
    if args.verify_receipts {
        bail!("offline packages do not contain upstream execution receipts");
    }
    if args.new_audit_at_checkpoint {
        bail!("offline replay does not permit skipping audit history at a checkpoint");
    }
    if args.offline_run_seconds == Some(0) {
        bail!("--offline-run-seconds must be positive");
    }
    let mut source = OfflineBlockSource::open(package)?;
    let target = match args.end {
        Some(target) if target > source.end => {
            bail!(
                "requested target {target} exceeds offline package end {}",
                source.end
            )
        }
        Some(target) => target,
        None => source.end,
    };
    let executor = NeoExecutor::open_with_batch_durability_checkpoint_replay(
        ProtocolSettings::mainnet(),
        &args.data_path,
        args.batch_durable,
    )?;
    executor.set_execution_mode(ExecutionMode::Canonical);
    executor.set_execution_profiling(args.profile_execution);
    executor.set_mpt_node_cache_enabled(!args.disable_mpt_node_cache);
    if let Some(capacity) = args.mpt_node_cache_capacity {
        executor.reset_mpt_node_cache_capacity(capacity);
    }
    let mut process_memory = args
        .profile_execution
        .then(ProcessMemoryMonitor::new)
        .transpose()?;
    let initial_height = executor.height()?;
    if initial_height.is_some_and(|height| height > target) {
        bail!("local height {initial_height:?} exceeds requested target {target}");
    }
    let checkpoint = executor.checkpoint()?;
    if initial_height.is_some() != checkpoint.is_some() {
        bail!("validator height and checkpoint availability disagree");
    }
    if let Some(parent) = args.audit_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut audit = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&args.audit_path)
        .with_context(|| format!("failed to open {}", args.audit_path.display()))?;
    validate_audit_resume(&mut audit, checkpoint.as_ref(), false)?;
    if let Some(checkpoint) = checkpoint.as_ref() {
        let record = read_last_audit_record(&mut audit)?;
        source.resume(checkpoint, &record)?;
    }
    let mut audit = BufWriter::new(audit);
    let started = Instant::now();
    let mut executed = 0_u64;
    let mut replay_profile = ReplayProfile::default();
    let next = initial_height.map_or(source.start, |height| height + 1);
    let output_dir = args.audit_path.parent().unwrap_or_else(|| Path::new("."));
    let result = replay_offline_range(
        &executor,
        &mut source,
        next,
        target,
        args.batch_size,
        args.batch_durable,
        !args.disable_empty_block_batch,
        args.offline_run_seconds,
        &mut audit,
        &mut executed,
        &mut replay_profile,
        &mut process_memory,
    );
    audit.flush()?;
    if let Err(error) = result {
        // A failed speculative batch poisons the executor. Do not query its
        // in-memory height or emit a success-shaped summary; durable storage
        // remains at the last completed physical batch boundary.
        executor.shutdown().await?;
        return Err(error);
    }
    let verified_through = executor.height()?;
    if let Some(monitor) = process_memory.as_mut() {
        monitor.sample()?;
    }
    let summary = OfflineRunSummary {
        network: "mainnet".to_owned(),
        source: package.display().to_string(),
        package_start: source.start,
        package_end: source.end,
        execution_mode: "canonical".to_owned(),
        local_replay_only: true,
        receipts_verified: false,
        batch_durable: args.batch_durable,
        empty_block_write_back: !args.disable_empty_block_batch,
        mpt_node_cache: executor.mpt_node_cache_stats().into(),
        process_memory: process_memory.as_ref().map(ProcessMemoryMonitor::profile),
        execution_profile: args
            .profile_execution
            .then(|| executor.execution_profile().into()),
        initial_height,
        verified_through,
        target_height: target,
        blocks_executed_this_run: executed,
        direct_transfers_this_run: executor.direct_transfer_count(),
        elapsed_seconds: started.elapsed().as_secs_f64(),
        replay_profile,
        complete_for_target: verified_through == Some(target),
    };
    fs::write(
        output_dir.join("last-run.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    executor.shutdown().await?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

fn replay_offline_range(
    executor: &NeoExecutor,
    source: &mut OfflineBlockSource,
    start: u32,
    target: u32,
    batch_size: u32,
    batch_durable: bool,
    empty_block_write_back: bool,
    run_seconds: Option<u64>,
    audit: &mut impl Write,
    executed: &mut u64,
    profile: &mut ReplayProfile,
    process_memory: &mut Option<ProcessMemoryMonitor>,
) -> Result<()> {
    if start > target {
        return Ok(());
    }
    let started = Instant::now();
    let mut batch_start = start;
    while batch_start <= target {
        let batch_end = batch_start.saturating_add(batch_size - 1).min(target);
        let stage = Instant::now();
        let batch = source.read_batch(batch_start, batch_end)?;
        profile.upstream_seconds += stage.elapsed().as_secs_f64();
        let result = process_offline_batch(
            executor,
            batch,
            !batch_durable,
            empty_block_write_back,
            audit,
            executed,
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
        result?;
        if let Some(monitor) = process_memory.as_mut() {
            monitor.sample()?;
        }
        println!(
            "offline-verified={batch_end} target={target} batch={batch_start}-{batch_end} executed={} rate={:.2} blocks/s read={:.2}s execute={:.2}s audit={:.2}s",
            *executed,
            *executed as f64 / started.elapsed().as_secs_f64().max(f64::EPSILON),
            profile.upstream_seconds,
            profile.local_execution_seconds,
            profile.audit_seconds,
        );
        batch_start = batch_end.checked_add(1).context("block height overflow")?;
        if run_seconds.is_some_and(|seconds| started.elapsed() >= Duration::from_secs(seconds)) {
            println!("offline-run-time-limit reached at a durable batch boundary");
            break;
        }
    }
    Ok(())
}

fn process_offline_batch(
    executor: &NeoExecutor,
    batch: Vec<OfflineBlock>,
    flush_each_block: bool,
    empty_block_write_back: bool,
    audit: &mut impl Write,
    executed: &mut u64,
    profile: &mut ReplayProfile,
) -> Result<()> {
    let decode_started = Instant::now();
    let mut prepared = Vec::with_capacity(batch.len());
    for item in batch {
        let block = decode_block(&item.raw, item.height)?;
        let block_hash = block.hash();
        prepared.push((item, block, block_hash));
    }
    profile.decode_compare_seconds += decode_started.elapsed().as_secs_f64();

    let mut offset = 0_usize;
    while offset < prepared.len() {
        let is_empty = prepared[offset].1.transactions.is_empty();
        let mut end = offset + 1;
        if empty_block_write_back && is_empty {
            while end < prepared.len()
                && prepared[end].1.transactions.is_empty()
                && end - offset < neo_system::NeoSystem::MAX_EMPTY_BLOCK_WRITE_BACK_BLOCKS
            {
                end += 1;
            }
        }
        let run = &prepared[offset..end];
        let first_height = run[0].0.height;
        let last_height = run[run.len() - 1].0.height;
        let stage = Instant::now();
        let outcomes = if run.len() >= 2 {
            let blocks = run.iter().map(|(_, block, _)| block.clone()).collect();
            executor.execute_empty_blocks(blocks).with_context(|| {
                format!(
                    "neo-rs failed to execute offline empty-block run {first_height}-{last_height}"
                )
            })?
        } else {
            vec![executor.execute(run[0].1.clone()).with_context(|| {
                format!("neo-rs failed to execute offline block {first_height}")
            })?]
        };
        profile.local_execution_seconds += stage.elapsed().as_secs_f64();
        if outcomes.len() != run.len() {
            bail!(
                "offline execution returned the wrong outcome count at {first_height}-{last_height}"
            );
        }

        let stage = Instant::now();
        for ((item, block, block_hash), outcome) in run.iter().zip(outcomes) {
            if outcome.block_hash != *block_hash {
                bail!(
                    "local block hash changed during execution at {}",
                    item.height
                );
            }
            serde_json::to_writer(
                &mut *audit,
                &AuditRecord {
                    height: item.height,
                    block_hash: block_hash.to_string(),
                    state_root: outcome.state_root.to_string(),
                    transaction_count: block.transactions.len(),
                    receipts_verified: false,
                    receipt_count: 0,
                    offline_block_offset: Some(item.block_offset),
                    offline_next_offset: Some(item.next_offset),
                },
            )?;
            audit.write_all(b"\n")?;
            if flush_each_block {
                audit.flush()?;
            }
            *executed += 1;
        }
        profile.audit_seconds += stage.elapsed().as_secs_f64();
        offset = end;
    }
    Ok(())
}
