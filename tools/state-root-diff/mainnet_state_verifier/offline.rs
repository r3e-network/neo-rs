use super::*;
#[derive(Debug)]
pub(super) struct OfflineBlock {
    pub(super) height: u32,
    pub(super) raw: Vec<u8>,
    pub(super) block_offset: u64,
    pub(super) next_offset: u64,
}

/// Strict reader for the format implemented by Neo.CLI MainService.Block.cs:
/// uint32 start, uint32 count, then count entries of int32 length + raw Block.
pub(super) struct OfflineBlockSource {
    file: File,
    file_len: u64,
    pub(super) start: u32,
    pub(super) end: u32,
    next_height: u32,
}

impl OfflineBlockSource {
    pub(super) fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)
            .with_context(|| format!("failed to open offline package {}", path.display()))?;
        let file_len = file.metadata()?.len();
        if file_len < 8 {
            bail!("offline package is shorter than its eight-byte header");
        }
        let start = read_u32_le(&mut file, "offline package start")?;
        let count = read_u32_le(&mut file, "offline package count")?;
        if start != 0 || count == 0 {
            bail!("full-chain offline replay requires a non-empty package starting at genesis");
        }
        let end = start
            .checked_add(count - 1)
            .context("offline package end height overflow")?;
        Ok(Self {
            file,
            file_len,
            start,
            end,
            next_height: start,
        })
    }

    pub(super) fn read_one(&mut self) -> Result<OfflineBlock> {
        if self.next_height > self.end {
            bail!("attempted to read beyond the declared offline package count");
        }
        let height = self.next_height;
        let block_offset = self.file.stream_position()?;
        let size = read_u32_le(&mut self.file, "offline block length")? as usize;
        if size == 0 || size > neo_system::network::p2p::PAYLOAD_MAX_SIZE {
            bail!("offline block {height} has invalid length {size}");
        }
        let mut raw = vec![0_u8; size];
        self.file
            .read_exact(&mut raw)
            .with_context(|| format!("offline block {height} is truncated"))?;
        let next_offset = self.file.stream_position()?;
        if height == self.end && next_offset != self.file_len {
            bail!("offline package has trailing bytes or an incorrect declared count");
        }
        self.next_height = self
            .next_height
            .checked_add(1)
            .context("offline block height overflow")?;
        Ok(OfflineBlock {
            height,
            raw,
            block_offset,
            next_offset,
        })
    }

    pub(super) fn read_batch(&mut self, start: u32, end: u32) -> Result<Vec<OfflineBlock>> {
        if start > end || start != self.next_height || end > self.end {
            bail!("offline batch is outside the declared sequential block range");
        }
        (start..=end).map(|_| self.read_one()).collect()
    }

    pub(super) fn resume(
        &mut self,
        checkpoint: &neo_validator::state::VerifiedPoint,
        record: &AuditRecord,
    ) -> Result<()> {
        let block_offset = record
            .offline_block_offset
            .context("offline audit checkpoint has no block offset")?;
        let expected_next = record
            .offline_next_offset
            .context("offline audit checkpoint has no next offset")?;
        self.file.seek(SeekFrom::Start(block_offset))?;
        self.next_height = checkpoint.height;
        let block = self.read_one()?;
        if block.block_offset != block_offset || block.next_offset != expected_next {
            bail!("offline audit offsets do not describe the checkpoint block");
        }
        let decoded = decode_block(&block.raw, checkpoint.height)?;
        if decoded.hash() != checkpoint.block_hash {
            bail!("offline package checkpoint block hash differs from the validator store");
        }
        Ok(())
    }
}

fn read_u32_le(file: &mut File, field: &str) -> Result<u32> {
    let mut bytes = [0_u8; 4];
    file.read_exact(&mut bytes)
        .with_context(|| format!("failed to read {field}"))?;
    Ok(u32::from_le_bytes(bytes))
}

pub(super) fn read_last_audit_record(file: &mut File) -> Result<AuditRecord> {
    let length = file.metadata()?.len();
    let tail_length = length.min(8192);
    file.seek(SeekFrom::End(-(tail_length as i64)))?;
    let mut tail = String::new();
    file.read_to_string(&mut tail)?;
    let line = tail
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .context("audit file has no final record")?;
    let record = serde_json::from_str(line).context("invalid final audit record")?;
    file.seek(SeekFrom::End(0))?;
    Ok(record)
}

fn read_direct_point(
    cache: &neo_storage::persistence::store_cache::StoreCache,
    height: u32,
) -> Result<neo_validator::state::VerifiedPoint> {
    use neo_native_contracts::ledger_contract::{HashOrIndex, LedgerContract};
    use neo_state_service::commit_handlers::read_state_root_record;

    let block = LedgerContract::new()
        .get_block(cache, HashOrIndex::Index(height))?
        .with_context(|| format!("persisted block {height} is missing"))?;
    if block.index() != height {
        bail!("persisted block height differs from its index at {height}");
    }
    let state_root = read_state_root_record(cache, height)
        .map_err(anyhow::Error::msg)?
        .with_context(|| format!("persisted state root {height} is missing"))?;
    Ok(neo_validator::state::VerifiedPoint {
        height,
        block_hash: block.hash(),
        state_root: state_root.root_hash,
    })
}

fn open_direct_checkpoint(
    data_path: &Path,
) -> Result<(
    neo_storage::persistence::store_cache::StoreCache,
    neo_validator::state::VerifiedPoint,
)> {
    use neo_native_contracts::ledger_contract::LedgerContract;
    use neo_storage::persistence::providers::rocksdb::RocksDBStoreProvider;
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::persistence::store_cache::StoreCache;
    use neo_storage::persistence::store_provider::StoreProvider;

    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: data_path.to_path_buf(),
        ..StorageConfig::default()
    });
    let store = provider.get_store("")?;
    let cache = StoreCache::new(store);
    let height = LedgerContract::new().current_index(&cache)?;
    let point = read_direct_point(&cache, height)?;
    Ok((cache, point))
}

pub(super) fn repair_offline_audit(args: &Args, package: &Path) -> Result<()> {
    use std::io::BufRead;

    let (cache, checkpoint) = open_direct_checkpoint(&args.data_path)?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&args.audit_path)
        .with_context(|| format!("failed to open {} for repair", args.audit_path.display()))?;
    let original_len = file.metadata()?.len();
    let mut reader = std::io::BufReader::new(file.try_clone()?);
    let mut valid_len = 0_u64;
    let mut expected_height = 0_u32;
    let mut last: Option<AuditRecord> = None;
    loop {
        let mut line = Vec::new();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        if !line.ends_with(b"\n") {
            break;
        }
        let record: AuditRecord = serde_json::from_slice(&line)
            .with_context(|| format!("invalid complete audit record at byte {valid_len}"))?;
        if record.height != expected_height {
            bail!("offline audit is not contiguous at height {expected_height}");
        }
        if record.offline_block_offset.is_none() || record.offline_next_offset.is_none() {
            bail!("non-offline audit record found at height {}", record.height);
        }
        if let Some(previous) = last.as_ref()
            && record.offline_block_offset != previous.offline_next_offset
        {
            bail!(
                "offline audit offsets are not contiguous at {}",
                record.height
            );
        }
        valid_len = valid_len
            .checked_add(read as u64)
            .context("audit length overflow")?;
        expected_height = expected_height
            .checked_add(1)
            .context("audit height overflow")?;
        last = Some(record);
    }
    drop(reader);
    let last = last.context("offline audit has no complete anchor record")?;
    let anchor = read_direct_point(&cache, last.height)?;
    if anchor.block_hash.to_string() != last.block_hash
        || anchor.state_root.to_string() != last.state_root
    {
        bail!("last complete audit record differs from persisted state");
    }
    if checkpoint.height < last.height {
        bail!("persisted checkpoint is behind the complete audit; refusing to truncate state");
    }
    let missing = checkpoint.height - last.height;
    if missing > 4096 {
        bail!("offline audit gap {missing} exceeds the explicit repair limit");
    }

    let mut repaired = Vec::new();
    if missing > 0 {
        let mut source = OfflineBlockSource::open(package)?;
        source.file.seek(SeekFrom::Start(
            last.offline_next_offset
                .context("offline anchor has no next offset")?,
        ))?;
        source.next_height = last.height + 1;
        for height in (last.height + 1)..=checkpoint.height {
            let item = source.read_one()?;
            let block = decode_block(&item.raw, height)?;
            let point = read_direct_point(&cache, height)?;
            if block.hash() != point.block_hash {
                bail!("package and persisted block hashes differ at {height}");
            }
            let mut encoded = serde_json::to_vec(&AuditRecord {
                height,
                block_hash: point.block_hash.to_string(),
                state_root: point.state_root.to_string(),
                transaction_count: block.transactions.len(),
                receipts_verified: false,
                receipt_count: 0,
                offline_block_offset: Some(item.block_offset),
                offline_next_offset: Some(item.next_offset),
            })?;
            encoded.push(b'\n');
            repaired.push(encoded);
        }
    }
    if valid_len == original_len && repaired.is_empty() {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "repair": "not_needed",
                "height": checkpoint.height,
            }))?
        );
        return Ok(());
    }

    let file_name = args
        .audit_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("offline audit filename is invalid")?;
    let backup = args
        .audit_path
        .with_file_name(format!("{file_name}.pre-repair-{}", checkpoint.height));
    if backup.exists() {
        bail!("repair backup already exists: {}", backup.display());
    }
    fs::copy(&args.audit_path, &backup)
        .with_context(|| format!("failed to back up audit to {}", backup.display()))?;
    file.set_len(valid_len)?;
    file.seek(SeekFrom::Start(valid_len))?;
    for record in &repaired {
        file.write_all(record)?;
    }
    file.sync_all()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "repair": "completed",
            "previous_complete_height": last.height,
            "persisted_checkpoint": checkpoint,
            "records_reconstructed": repaired.len(),
            "trailing_bytes_removed": original_len - valid_len,
            "backup": backup,
        }))?
    );
    Ok(())
}
