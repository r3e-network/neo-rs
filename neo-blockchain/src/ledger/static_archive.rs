//! # neo-blockchain::ledger::static_archive
//!
//! Append-only cold ledger archive for finalized block and transaction bodies.
//!
//! ## Boundary
//!
//! This module belongs to `neo-blockchain`. It owns the ledger-specific cold
//! block/transaction archive and hot/cold read router, but it does not replace
//! the canonical native Ledger records or change block-import persistence.
//!
//! ## Contents
//!
//! - `StaticLedgerArchive`: append-only cold archive for block and transaction
//!   bodies.
//! - `HotColdLedgerProvider`: provider router that reads cold history first and
//!   falls back to hot Ledger records.

use super::ledger_provider::{BlockProvider, TxProvider};
use neo_error::{CoreError, CoreResult};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_payloads::{Block, Header, Transaction};
use neo_primitives::UInt256;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const BLOCK_DATA_FILE: &str = "blocks.dat";
const BLOCK_HASH_INDEX_FILE: &str = "blocks-by-hash.idx";
const BLOCK_HEIGHT_INDEX_FILE: &str = "blocks-by-height.idx";
const TX_DATA_FILE: &str = "transactions.dat";
const TX_INDEX_FILE: &str = "transactions-by-hash.idx";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RecordPointer {
    offset: u64,
    len: u32,
}

#[derive(Debug, Default)]
struct StaticLedgerArchiveIndex {
    blocks_by_hash: HashMap<UInt256, RecordPointer>,
    block_hashes_by_height: HashMap<u32, UInt256>,
    transactions_by_hash: HashMap<UInt256, RecordPointer>,
}

/// Append-only static files for finalized block and transaction bodies.
///
/// The hot `LedgerContract` records remain the canonical C#-compatible state
/// layout. This archive gives read paths a cold/history body store that can be
/// composed with the hot provider boundary and later used to shrink hot DB
/// retention safely.
pub struct StaticLedgerArchive {
    path: PathBuf,
    index: Mutex<StaticLedgerArchiveIndex>,
}

impl StaticLedgerArchive {
    /// Opens or creates an archive under `path`, rebuilding in-memory indexes
    /// from the append-only index files.
    pub fn open(path: impl AsRef<Path>) -> CoreResult<Self> {
        let path = path.as_ref().to_path_buf();
        fs::create_dir_all(&path)
            .map_err(|err| io_error("create static ledger archive", &path, err))?;
        let index = StaticLedgerArchiveIndex {
            blocks_by_hash: load_hash_index(&path.join(BLOCK_HASH_INDEX_FILE))?,
            block_hashes_by_height: load_height_index(&path.join(BLOCK_HEIGHT_INDEX_FILE))?,
            transactions_by_hash: load_hash_index(&path.join(TX_INDEX_FILE))?,
        };
        Ok(Self {
            path,
            index: Mutex::new(index),
        })
    }

    /// Appends `block` and its transactions to the cold archive.
    ///
    /// Re-appending the same block or transaction is idempotent; attempting to
    /// bind an already-archived height to a different block hash is rejected.
    pub fn append_block(&self, block: &Block) -> CoreResult<UInt256> {
        let block_hash = block.try_hash()?;
        let block_index = block.index();
        let block_bytes = serialize(block)?;
        let mut tx_records = Vec::with_capacity(block.transactions.len());
        for tx in &block.transactions {
            let tx_hash = tx.try_hash()?;
            tx_records.push((tx_hash, serialize(tx)?));
        }

        let mut index = self.index.lock();
        if let Some(existing_hash) = index.block_hashes_by_height.get(&block_index)
            && *existing_hash != block_hash
        {
            return Err(CoreError::invalid_operation(format!(
                "static ledger archive height {block_index} already points to {existing_hash}, not {block_hash}"
            )));
        }

        if let std::collections::hash_map::Entry::Vacant(entry) =
            index.blocks_by_hash.entry(block_hash)
        {
            let pointer = append_payload(&self.path.join(BLOCK_DATA_FILE), &block_bytes)?;
            append_hash_index(&self.path.join(BLOCK_HASH_INDEX_FILE), &block_hash, pointer)?;
            entry.insert(pointer);
        }
        if let std::collections::hash_map::Entry::Vacant(entry) =
            index.block_hashes_by_height.entry(block_index)
        {
            append_height_index(
                &self.path.join(BLOCK_HEIGHT_INDEX_FILE),
                block_index,
                &block_hash,
            )?;
            entry.insert(block_hash);
        }

        for (tx_hash, tx_bytes) in tx_records {
            if index.transactions_by_hash.contains_key(&tx_hash) {
                continue;
            }
            let pointer = append_payload(&self.path.join(TX_DATA_FILE), &tx_bytes)?;
            append_hash_index(&self.path.join(TX_INDEX_FILE), &tx_hash, pointer)?;
            index.transactions_by_hash.insert(tx_hash, pointer);
        }

        Ok(block_hash)
    }

    fn read_block(&self, pointer: RecordPointer) -> CoreResult<Block> {
        read_payload(&self.path.join(BLOCK_DATA_FILE), pointer)
    }

    fn read_transaction(&self, pointer: RecordPointer) -> CoreResult<Transaction> {
        read_payload(&self.path.join(TX_DATA_FILE), pointer)
    }
}

impl BlockProvider for StaticLedgerArchive {
    fn block_hash_by_index(&self, index: u32) -> CoreResult<Option<UInt256>> {
        Ok(self
            .index
            .lock()
            .block_hashes_by_height
            .get(&index)
            .copied())
    }

    fn header_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Header>> {
        Ok(self.block_by_hash(hash)?.map(|block| block.header))
    }

    fn block_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Block>> {
        let pointer = self.index.lock().blocks_by_hash.get(hash).copied();
        pointer.map(|pointer| self.read_block(pointer)).transpose()
    }
}

impl TxProvider for StaticLedgerArchive {
    fn transaction_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Transaction>> {
        let pointer = self.index.lock().transactions_by_hash.get(hash).copied();
        pointer
            .map(|pointer| self.read_transaction(pointer))
            .transpose()
    }
}

/// Provider that composes hot ledger indexes with a cold static archive.
pub struct HotColdLedgerProvider<H, C> {
    hot: H,
    cold: C,
}

impl<H, C> HotColdLedgerProvider<H, C> {
    /// Creates a provider over a hot provider plus cold archive/provider.
    pub const fn new(hot: H, cold: C) -> Self {
        Self { hot, cold }
    }
}

impl<H, C> BlockProvider for HotColdLedgerProvider<H, C>
where
    H: BlockProvider,
    C: BlockProvider,
{
    fn block_hash_by_index(&self, index: u32) -> CoreResult<Option<UInt256>> {
        match self.hot.block_hash_by_index(index)? {
            Some(hash) => Ok(Some(hash)),
            None => self.cold.block_hash_by_index(index),
        }
    }

    fn header_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Header>> {
        match self.cold.header_by_hash(hash)? {
            Some(header) => Ok(Some(header)),
            None => self.hot.header_by_hash(hash),
        }
    }

    fn block_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Block>> {
        match self.cold.block_by_hash(hash)? {
            Some(block) => Ok(Some(block)),
            None => self.hot.block_by_hash(hash),
        }
    }
}

impl<H, C> TxProvider for HotColdLedgerProvider<H, C>
where
    H: TxProvider,
    C: TxProvider,
{
    fn transaction_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Transaction>> {
        match self.cold.transaction_by_hash(hash)? {
            Some(tx) => Ok(Some(tx)),
            None => self.hot.transaction_by_hash(hash),
        }
    }
}

fn serialize<T: Serializable>(value: &T) -> CoreResult<Vec<u8>> {
    let mut writer = BinaryWriter::with_capacity(value.size());
    value.serialize(&mut writer).map_err(|err| {
        CoreError::serialization(format!("static ledger archive serialize: {err}"))
    })?;
    Ok(writer.into_bytes())
}

fn deserialize<T: Serializable>(bytes: &[u8], path: &Path) -> CoreResult<T> {
    let mut reader = MemoryReader::new(bytes);
    T::deserialize(&mut reader)
        .map_err(|err| CoreError::deserialization(format!("{}: {err}", path.display())))
}

fn append_payload(path: &Path, bytes: &[u8]) -> CoreResult<RecordPointer> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)
        .map_err(|err| io_error("open archive data file", path, err))?;
    let offset = file
        .seek(SeekFrom::End(0))
        .map_err(|err| io_error("seek archive data file", path, err))?;
    file.write_all(bytes)
        .map_err(|err| io_error("append archive data file", path, err))?;
    let len = u32::try_from(bytes.len()).map_err(|_| {
        CoreError::invalid_operation(format!(
            "static ledger archive payload too large: {} bytes",
            bytes.len()
        ))
    })?;
    Ok(RecordPointer { offset, len })
}

fn read_payload<T: Serializable>(path: &Path, pointer: RecordPointer) -> CoreResult<T> {
    let mut file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|err| io_error("open archive data file", path, err))?;
    file.seek(SeekFrom::Start(pointer.offset))
        .map_err(|err| io_error("seek archive data file", path, err))?;
    let mut bytes = vec![0u8; pointer.len as usize];
    file.read_exact(&mut bytes)
        .map_err(|err| io_error("read archive data file", path, err))?;
    deserialize(&bytes, path)
}

fn append_hash_index(path: &Path, hash: &UInt256, pointer: RecordPointer) -> CoreResult<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| io_error("open archive hash index", path, err))?;
    file.write_all(&hash.to_bytes())
        .and_then(|_| file.write_all(&pointer.offset.to_le_bytes()))
        .and_then(|_| file.write_all(&pointer.len.to_le_bytes()))
        .map_err(|err| io_error("append archive hash index", path, err))
}

fn append_height_index(path: &Path, height: u32, hash: &UInt256) -> CoreResult<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| io_error("open archive height index", path, err))?;
    file.write_all(&height.to_le_bytes())
        .and_then(|_| file.write_all(&hash.to_bytes()))
        .map_err(|err| io_error("append archive height index", path, err))
}

fn load_hash_index(path: &Path) -> CoreResult<HashMap<UInt256, RecordPointer>> {
    let Some(bytes) = read_optional(path)? else {
        return Ok(HashMap::new());
    };
    const ENTRY_LEN: usize = 32 + 8 + 4;
    if bytes.len() % ENTRY_LEN != 0 {
        return Err(CoreError::invalid_data(format!(
            "static ledger hash index {} has truncated entry",
            path.display()
        )));
    }
    let mut index = HashMap::new();
    for entry in bytes.chunks_exact(ENTRY_LEN) {
        let hash = UInt256::from_bytes(&entry[..32])
            .map_err(|err| CoreError::invalid_data(format!("{}: {err}", path.display())))?;
        let offset = u64::from_le_bytes(entry[32..40].try_into().expect("slice length"));
        let len = u32::from_le_bytes(entry[40..44].try_into().expect("slice length"));
        index.insert(hash, RecordPointer { offset, len });
    }
    Ok(index)
}

fn load_height_index(path: &Path) -> CoreResult<HashMap<u32, UInt256>> {
    let Some(bytes) = read_optional(path)? else {
        return Ok(HashMap::new());
    };
    const ENTRY_LEN: usize = 4 + 32;
    if bytes.len() % ENTRY_LEN != 0 {
        return Err(CoreError::invalid_data(format!(
            "static ledger height index {} has truncated entry",
            path.display()
        )));
    }
    let mut index = HashMap::new();
    for entry in bytes.chunks_exact(ENTRY_LEN) {
        let height = u32::from_le_bytes(entry[..4].try_into().expect("slice length"));
        let hash = UInt256::from_bytes(&entry[4..])
            .map_err(|err| CoreError::invalid_data(format!("{}: {err}", path.display())))?;
        index.insert(height, hash);
    }
    Ok(index)
}

fn read_optional(path: &Path) -> CoreResult<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(io_error("read archive index", path, err)),
    }
}

fn io_error(action: &str, path: &Path, err: std::io::Error) -> CoreError {
    CoreError::io(format!("{action} {}: {err}", path.display()))
}

#[cfg(test)]
#[path = "../tests/ledger/static_archive.rs"]
mod tests;
