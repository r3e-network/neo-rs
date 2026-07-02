//! Pruning readiness checkpoints for hot/cold ledger storage.
//!
//! This module does not delete hot ledger records. It records consumer
//! acknowledgements and computes the highest height that a future pruning worker
//! may consider after checking retention and cold-archive coverage.

use super::ledger_provider::BlockProvider;
use neo_error::{CoreError, CoreResult};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const PRUNING_ACK_FILE: &str = "pruning-consumer-acks.idx";

/// Result of evaluating whether hot ledger history is safe to prune.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PruningReadiness {
    /// No consumer has acknowledged a processed height yet.
    NoConsumers,
    /// Consumers are caught up, but the configured retention window keeps all
    /// currently acknowledged history hot.
    RetainedByPolicy {
        /// Consumer that currently limits pruning.
        limiting_consumer: String,
        /// Height acknowledged by the limiting consumer.
        acknowledged_height: u32,
        /// Retention window in blocks.
        retention_blocks: u32,
    },
    /// The acknowledgement window allows pruning, but the cold archive is not
    /// complete for the target range.
    WaitingForArchive {
        /// Candidate height through which pruning would otherwise be allowed.
        prune_through_height: u32,
        /// First missing archived block height in the candidate range.
        missing_height: u32,
    },
    /// All consumers and archive coverage allow pruning through this height.
    Ready {
        /// Highest hot ledger block height that may be pruned.
        prune_through_height: u32,
        /// Consumer that currently limits pruning.
        limiting_consumer: String,
        /// Height acknowledged by the limiting consumer.
        acknowledged_height: u32,
        /// Retention window in blocks.
        retention_blocks: u32,
    },
}

/// Append-only store of canonical-chain consumer acknowledgement heights.
pub struct LedgerPruningStore {
    path: PathBuf,
    acknowledgements: Mutex<BTreeMap<String, u32>>,
}

impl LedgerPruningStore {
    /// Opens a pruning checkpoint store under `path`.
    pub fn open(path: impl AsRef<Path>) -> CoreResult<Self> {
        let path = path.as_ref().to_path_buf();
        fs::create_dir_all(&path)
            .map_err(|err| io_error("create ledger pruning store", &path, err))?;
        let acknowledgements = load_acknowledgements(&path.join(PRUNING_ACK_FILE))?;
        Ok(Self {
            path,
            acknowledgements: Mutex::new(acknowledgements),
        })
    }

    /// Records that `consumer` has durably processed through `height`.
    pub fn acknowledge(&self, consumer: impl AsRef<str>, height: u32) -> CoreResult<()> {
        let consumer = normalize_consumer(consumer.as_ref())?;
        let mut acknowledgements = self.acknowledgements.lock();
        if acknowledgements
            .get(&consumer)
            .is_some_and(|acknowledged| *acknowledged >= height)
        {
            return Ok(());
        }
        append_acknowledgement(&self.path.join(PRUNING_ACK_FILE), &consumer, height)?;
        acknowledgements.insert(consumer, height);
        Ok(())
    }

    /// Returns the last acknowledged height for `consumer`.
    pub fn acknowledged_height(&self, consumer: &str) -> Option<u32> {
        self.acknowledgements.lock().get(consumer).copied()
    }

    /// Returns whether hot ledger data can be pruned after applying retention
    /// and verifying cold archive coverage.
    pub fn readiness<P>(&self, archive: &P, retention_blocks: u32) -> CoreResult<PruningReadiness>
    where
        P: BlockProvider,
    {
        let acknowledgements = self.acknowledgements.lock();
        let Some((limiting_consumer, acknowledged_height)) = acknowledgements.iter().min_by(
            |(left_consumer, left_height), (right_consumer, right_height)| {
                left_height
                    .cmp(right_height)
                    .then_with(|| left_consumer.cmp(right_consumer))
            },
        ) else {
            return Ok(PruningReadiness::NoConsumers);
        };

        let prune_through_height = acknowledged_height.saturating_sub(retention_blocks);
        if prune_through_height == 0 {
            return Ok(PruningReadiness::RetainedByPolicy {
                limiting_consumer: limiting_consumer.clone(),
                acknowledged_height: *acknowledged_height,
                retention_blocks,
            });
        }

        for height in 1..=prune_through_height {
            if archive.block_hash_by_index(height)?.is_none() {
                return Ok(PruningReadiness::WaitingForArchive {
                    prune_through_height,
                    missing_height: height,
                });
            }
        }

        Ok(PruningReadiness::Ready {
            prune_through_height,
            limiting_consumer: limiting_consumer.clone(),
            acknowledged_height: *acknowledged_height,
            retention_blocks,
        })
    }
}

fn normalize_consumer(consumer: &str) -> CoreResult<String> {
    let consumer = consumer.trim();
    if consumer.is_empty() {
        return Err(CoreError::invalid_operation(
            "ledger pruning consumer id must not be empty",
        ));
    }
    if consumer.len() > u16::MAX as usize {
        return Err(CoreError::invalid_operation(format!(
            "ledger pruning consumer id too long: {} bytes",
            consumer.len()
        )));
    }
    Ok(consumer.to_string())
}

fn append_acknowledgement(path: &Path, consumer: &str, height: u32) -> CoreResult<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| io_error("open ledger pruning ack file", path, err))?;
    let len = u16::try_from(consumer.len()).map_err(|_| {
        CoreError::invalid_operation(format!(
            "ledger pruning consumer id too long: {} bytes",
            consumer.len()
        ))
    })?;
    file.write_all(&len.to_le_bytes())
        .and_then(|_| file.write_all(consumer.as_bytes()))
        .and_then(|_| file.write_all(&height.to_le_bytes()))
        .map_err(|err| io_error("append ledger pruning ack file", path, err))
}

fn load_acknowledgements(path: &Path) -> CoreResult<BTreeMap<String, u32>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => return Err(io_error("read ledger pruning ack file", path, err)),
    };

    let mut acknowledgements = BTreeMap::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        if bytes.len().saturating_sub(offset) < 2 {
            return Err(CoreError::invalid_data(format!(
                "ledger pruning ack file {} has truncated consumer length",
                path.display()
            )));
        }
        let len = u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("slice length"))
            as usize;
        offset += 2;

        if len == 0 {
            return Err(CoreError::invalid_data(format!(
                "ledger pruning ack file {} has empty consumer id",
                path.display()
            )));
        }
        if bytes.len().saturating_sub(offset) < len + 4 {
            return Err(CoreError::invalid_data(format!(
                "ledger pruning ack file {} has truncated acknowledgement record",
                path.display()
            )));
        }

        let consumer = std::str::from_utf8(&bytes[offset..offset + len])
            .map_err(|err| CoreError::invalid_data(format!("{}: {err}", path.display())))?
            .to_string();
        offset += len;
        let height =
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("slice length"));
        offset += 4;
        acknowledgements.insert(consumer, height);
    }

    Ok(acknowledgements)
}

fn io_error(action: &str, path: &Path, err: std::io::Error) -> CoreError {
    CoreError::io(format!("{action} {}: {err}", path.display()))
}
