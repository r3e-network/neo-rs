use super::env::*;
use super::run::*;
use crate::import_batching::{BlockMeta, DrainTrigger, validate_empty_run};
use crate::import_telemetry::LogsDeltas;
use anyhow::{Context, Result, bail};
use neo_primitives::UInt256;
use neo_rpc::application_logs::{
    ApplicationLogsService, ImportSessionOutcome, PersistenceUnitOutcome,
};
use neo_system::NeoSystem;
use std::sync::Arc;
use tracing::warn;

/// Brackets one chain commit with an ApplicationLogs persistence unit.
///
/// Records the committed callbacks produce for the blocks of this unit are
/// staged instead of written, so a chain commit that fails can discard them: the
/// logs database then never claims a block the chain store refused. A unit is
/// opened for every block (and every coalesced run) and closed before the next
/// decode begins.
///
/// Closing a unit only *promotes* its records. Inside an import session the
/// commit happens at the next checkpoint, which is what turns one logs write per
/// block into one logs write per checkpoint interval.
pub(super) struct LogsUnit {
    logs: Option<Arc<ApplicationLogsService>>,
}

impl LogsUnit {
    pub(super) fn begin(system: &Arc<NeoSystem>) -> Result<Self> {
        let logs = system
            .get_service::<ApplicationLogsService>()
            .ok()
            .flatten();
        if let Some(logs) = logs.as_ref() {
            match logs.begin_persistence_unit() {
                PersistenceUnitOutcome::Started => {}
                other => bail!("application logs persistence unit did not start: {other:?}"),
            }
        }
        Ok(Self { logs })
    }

    /// Promotes this unit's records to the session's accepted set.
    ///
    /// Nothing is committed here: the checkpoint decides when the accumulated
    /// records become durable, and the session's byte cap can force an earlier
    /// commit when the pending set would otherwise grow without bound.
    pub(super) fn finish(self) -> Result<()> {
        let Some(logs) = self.logs else {
            return Ok(());
        };
        match logs
            .finish_persistence_unit()
            .map_err(|err| anyhow::anyhow!(err))?
        {
            PersistenceUnitOutcome::Finished { .. } => Ok(()),
            other => bail!("application logs persistence unit did not finish: {other:?}"),
        }
    }

    pub(super) fn abort(self) {
        if let Some(logs) = self.logs {
            logs.abort_persistence_unit();
        }
    }
}
/// Import-scoped ApplicationLogs session.
///
/// Opening the session switches the service into batching mode: a finished unit
/// promotes its records and only a checkpoint commits them. The session is also
/// what makes the logs watermark mean "durable up to this height", so the
/// checkpoint can check it against the chain.
pub(super) struct LogsSession {
    logs: Option<Arc<ApplicationLogsService>>,
}

impl LogsSession {
    /// Opens the session, or does nothing when no logs service is registered.
    pub(super) fn begin(
        system: &Arc<NeoSystem>,
        chain_height: u32,
        chain_hash: &UInt256,
    ) -> Result<Self> {
        let logs = system
            .get_service::<ApplicationLogsService>()
            .ok()
            .flatten();
        if let Some(logs) = logs.as_ref() {
            logs.set_accepted_byte_cap(import_logs_byte_cap_from_env());
            match logs.begin_import_session() {
                ImportSessionOutcome::Started => {}
                other => bail!("application logs import session did not start: {other:?}"),
            }
            // Known limitation: a prefix whose logs were cut short by a crash
            // between the chain flush and the logs flush is detected here but not
            // repaired. Full resume validation is future work, so the import
            // continues and the gap stays visible in the warning.
            if chain_height > 0 && logs.get_block_log(chain_hash).is_none() {
                warn!(
                    target: "neo",
                    chain_height,
                    chain_hash = %chain_hash,
                    "application logs hold no record for the local chain tip; the logs store trails the chain and resume validation is not implemented yet (continuing)"
                );
            }
        }
        Ok(Self { logs })
    }

    /// Commits whatever is still pending and leaves batching mode.
    ///
    /// Returns the session's logs telemetry so the import summary can report it.
    /// The counters stay readable after the session ends, which is why they are
    /// read here instead of being reset.
    pub(super) fn end(self) -> Result<LogsDeltas> {
        let Some(logs) = self.logs else {
            return Ok(LogsDeltas::default());
        };
        match logs
            .end_import_session()
            .map_err(|err| anyhow::anyhow!(err))?
        {
            ImportSessionOutcome::Ended { .. } | ImportSessionOutcome::NotOpen => {}
            other => bail!("application logs import session did not end: {other:?}"),
        }
        let telemetry = logs.flush_telemetry();
        Ok(LogsDeltas {
            flush_count: telemetry.flushes,
            flushed_records: telemetry.records,
            flushed_bytes: telemetry.bytes,
            pending_records: telemetry.pending_records,
        })
    }
}

/// Rejects a block that does not name the accepted tip as its predecessor.
fn preflight_link(parent: Option<UInt256>, meta: &BlockMeta) -> Result<()> {
    if let Some(parent) = parent
        && meta.prev_hash != parent
    {
        bail!(
            "block {} does not extend the accepted tip: prev_hash {} does not match {}",
            meta.index,
            meta.prev_hash,
            parent
        );
    }
    Ok(())
}

/// Persists one decoded block through the single-block path inside a unit.
pub(super) fn persist_single_block(
    system: &Arc<NeoSystem>,
    parent: Option<UInt256>,
    decoded: DecodedBlock,
) -> Result<UnitOutcome> {
    let meta = decoded.meta;
    preflight_link(parent, &meta)?;
    let unit = LogsUnit::begin(system)?;
    match system.persist_block_without_runtime_cache(decoded.block) {
        Ok(_) => {
            unit.finish()
                .with_context(|| format!("failed to record the logs of block {}", meta.index))?;
            Ok(UnitOutcome {
                accepted: 1,
                height: meta.index,
                hash: meta.hash,
            })
        }
        Err(err) => {
            // A failure after execution started poisons the system: it must not
            // be retried, and no serial fallback may run. The unit is aborted so
            // nothing is left claiming this block.
            let poisoned = system.is_persistence_poisoned();
            unit.abort();
            Err(anyhow::anyhow!(err)).with_context(|| {
                format!(
                    "failed to persist imported block {} (persistence poisoned: {poisoned})",
                    meta.index
                )
            })
        }
    }
}

/// Persists a decoded run of transaction-free blocks as one write-back.
///
/// The run is validated *before* the persistence layer starts speculative
/// execution, so a run that is not a contiguous, predecessor-linked chain of
/// transaction-free blocks is rejected without poisoning the system.
pub(super) fn persist_empty_run(
    system: &Arc<NeoSystem>,
    parent: Option<UInt256>,
    run: Vec<DecodedBlock>,
    trigger: DrainTrigger,
) -> Result<UnitOutcome> {
    let metas = run.iter().map(|decoded| decoded.meta).collect::<Vec<_>>();
    let last = *metas
        .last()
        .expect("a drained run always contains at least one block");
    validate_empty_run(parent, &metas)
        .map_err(|err| anyhow::anyhow!(err))
        .with_context(|| {
            format!(
                "empty-block run ending at {} rejected before persistence ({})",
                last.index,
                trigger.as_str()
            )
        })?;
    let accepted = u64::try_from(metas.len()).unwrap_or(u64::MAX);
    let blocks = run.into_iter().map(|decoded| decoded.block).collect();

    let unit = LogsUnit::begin(system)?;
    match system.persist_consecutive_empty_blocks_without_runtime_cache(blocks) {
        Ok(_) => {
            unit.finish().with_context(|| {
                format!(
                    "failed to record the logs of the {accepted} empty blocks ending at {}",
                    last.index
                )
            })?;
            Ok(UnitOutcome {
                accepted,
                height: last.index,
                hash: last.hash,
            })
        }
        Err(err) => {
            let poisoned = system.is_persistence_poisoned();
            unit.abort();
            Err(anyhow::anyhow!(err)).with_context(|| {
                format!(
                    "failed to persist {accepted} empty blocks ending at {} (persistence poisoned: {poisoned})",
                    last.index
                )
            })
        }
    }
}
