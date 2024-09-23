use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::error::Error;
use std::fmt;

use crate::config::limits;
use crate::core::block;
use crate::core::state;
use crate::core::storage;
use crate::core::transaction;
use crate::encoding::address;
use crate::encoding::bigint;
use crate::io;
use crate::smartcontract::trigger;
use crate::util;
use crate::vm::stackitem;

// HasTransaction errors.
#[derive(Debug)]
pub enum DaoError {
    AlreadyExists,
    HasConflicts,
    InternalDBInconsistency,
}

impl fmt::Display for DaoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DaoError::AlreadyExists => write!(f, "transaction already exists"),
            DaoError::HasConflicts => write!(f, "transaction has conflicts"),
            DaoError::InternalDBInconsistency => write!(f, "internal DB inconsistency"),
        }
    }
}

impl Error for DaoError {}

// conflictRecordValueLen is the length of value of transaction conflict record.
// It consists of 1-byte [storage.ExecTransaction] prefix and 4-bytes block index
// in the LE form.
const CONFLICT_RECORD_VALUE_LEN: usize = 1 + 4;

// Simple is memCached wrapper around DB, simple DAO implementation.
pub struct Simple {
    version: Version,
    store: Arc<storage::MemCachedStore>,

    native_cache_lock: RwLock<()>,
    native_cache: HashMap<i32, Box<dyn NativeContractCache>>,
    native_cache_ps: Option<Arc<Simple>>,

    private: AtomicBool,
    ser_ctx: Mutex<Option<stackitem::SerializationContext>>,
    key_buf: Mutex<Vec<u8>>,
    data_buf: Mutex<io::BufBinWriter>,
}

// NativeContractCache is an interface representing cache for a native contract.
// Cache can be copied to create a wrapper around current DAO layer. Wrapped cache
// can be persisted to the underlying DAO native cache.
pub trait NativeContractCache: Send + Sync {
    // Copy returns a copy of native cache item that can safely be changed within
    // the subsequent DAO operations.
    fn copy(&self) -> Box<dyn NativeContractCache>;
}

// NewSimple creates a new simple dao using the provided backend store.
pub fn new_simple(backend: Arc<dyn storage::Store>, state_root_in_header: bool) -> Arc<Simple> {
    let st = storage::new_mem_cached_store(backend);
    new_simple_with_store(st, state_root_in_header)
}

fn new_simple_with_store(st: Arc<storage::MemCachedStore>, state_root_in_header: bool) -> Arc<Simple> {
    Arc::new(Simple {
        version: Version {
            storage_prefix: storage::KeyPrefix::STStorage,
            state_root_in_header,
        },
        store: st,
        native_cache: HashMap::new(),
        native_cache_ps: None,
        private: AtomicBool::new(false),
        ser_ctx: Mutex::new(None),
        key_buf: Mutex::new(Vec::new()),
        data_buf: Mutex::new(io::BufBinWriter::new()),
    })
}

// GetBatch returns the currently accumulated DB changeset.
pub fn get_batch(dao: &Simple) -> Arc<storage::MemBatch> {
    dao.store.get_batch()
}

// GetWrapped returns a new DAO instance with another layer of wrapped
// MemCachedStore around the current DAO Store.
pub fn get_wrapped(dao: &Simple) -> Arc<Simple> {
    let d = new_simple(dao.store.clone(), dao.version.state_root_in_header);
    d.version = dao.version.clone();
    d.native_cache_ps = Some(Arc::clone(dao));
    d
}

// GetPrivate returns a new DAO instance with another layer of private
// MemCachedStore around the current DAO Store.
pub fn get_private(dao: &Simple) -> Arc<Simple> {
    let d = Arc::new(Simple {
        version: dao.version.clone(),
        key_buf: dao.key_buf.lock().unwrap().clone(),
        data_buf: Mutex::new(io::BufBinWriter::new()),
        ser_ctx: Mutex::new(None),
        store: storage::new_private_mem_cached_store(dao.store.clone()),
        private: AtomicBool::new(true),
        native_cache_ps: Some(Arc::clone(dao)),
        native_cache: HashMap::new(),
    });
    d
}

// GetAndDecode performs get operation and decoding with serializable structures.
pub fn get_and_decode(dao: &Simple, entity: &mut dyn io::Serializable, key: &[u8]) -> Result<(), Box<dyn Error>> {
    let entity_bytes = dao.store.get(key)?;
    let mut reader = io::BinReader::new(&entity_bytes);
    entity.decode_binary(&mut reader)?;
    Ok(())
}

// putWithBuffer performs put operation using buf as a pre-allocated buffer for serialization.
pub fn put_with_buffer(dao: &Simple, entity: &dyn io::Serializable, key: &[u8], buf: &mut io::BufBinWriter) -> Result<(), Box<dyn Error>> {
    entity.encode_binary(buf)?;
    if buf.has_error() {
        return Err(Box::new(buf.get_error().unwrap()));
    }
    dao.store.put(key, buf.bytes());
    Ok(())
}

// -- start NEP-17 transfer info.

pub fn make_tti_key(dao: &Simple, acc: &util::Uint160) -> Vec<u8> {
    let mut key = dao.get_key_buf(1 + util::UINT160_SIZE);
    key[0] = storage::KeyPrefix::STTokenTransferInfo as u8;
    key[1..].copy_from_slice(&acc.bytes_be());
    key
}

// GetTokenTransferInfo retrieves NEP-17 transfer info from the cache.
pub fn get_token_transfer_info(dao: &Simple, acc: &util::Uint160) -> Result<state::TokenTransferInfo, Box<dyn Error>> {
    let key = make_tti_key(dao, acc);
    let mut bs = state::TokenTransferInfo::new();
    match get_and_decode(dao, &mut bs, &key) {
        Ok(_) => Ok(bs),
        Err(e) if e.downcast_ref::<storage::StorageError>() == Some(&storage::StorageError::KeyNotFound) => Ok(bs),
        Err(e) => Err(e),
    }
}

// PutTokenTransferInfo saves NEP-17 transfer info in the cache.
pub fn put_token_transfer_info(dao: &Simple, acc: &util::Uint160, bs: &state::TokenTransferInfo) -> Result<(), Box<dyn Error>> {
    put_token_transfer_info_with_buf(dao, acc, bs, &mut dao.get_data_buf())
}

fn put_token_transfer_info_with_buf(dao: &Simple, acc: &util::Uint160, bs: &state::TokenTransferInfo, buf: &mut io::BufBinWriter) -> Result<(), Box<dyn Error>> {
    put_with_buffer(dao, bs, &make_tti_key(dao, acc), buf)
}

// -- end NEP-17 transfer info.

// -- start transfer log.

pub fn get_token_transfer_log_key(dao: &Simple, acc: &util::Uint160, newest_timestamp: u64, index: u32, is_nep11: bool) -> Vec<u8> {
    let mut key = dao.get_key_buf(1 + util::UINT160_SIZE + 8 + 4);
    key[0] = if is_nep11 { storage::KeyPrefix::STNEP11Transfers as u8 } else { storage::KeyPrefix::STNEP17Transfers as u8 };
    key[1..1 + util::UINT160_SIZE].copy_from_slice(&acc.bytes_be());
    key[1 + util::UINT160_SIZE..1 + util::UINT160_SIZE + 8].copy_from_slice(&newest_timestamp.to_be_bytes());
    key[1 + util::UINT160_SIZE + 8..].copy_from_slice(&index.to_be_bytes());
    key
}

// SeekNEP17TransferLog executes f for each NEP-17 transfer in log starting from
// the transfer with the newest timestamp up to the oldest transfer. It continues
// iteration until false is returned from f. The last non-nil error is returned.
pub fn seek_nep17_transfer_log<F>(dao: &Simple, acc: &util::Uint160, newest_timestamp: u64, mut f: F) -> Result<(), Box<dyn Error>>
where
    F: FnMut(&state::NEP17Transfer) -> Result<bool, Box<dyn Error>>,
{
    let key = get_token_transfer_log_key(dao, acc, newest_timestamp, 0, false);
    let prefix_len = 1 + util::UINT160_SIZE;
    let mut seek_err: Option<Box<dyn Error>> = None;
    dao.store.seek(storage::SeekRange {
        prefix: &key[..prefix_len],
        start: &key[prefix_len..prefix_len + 8],
        backwards: true,
    }, |k, v| {
        let mut lg = state::TokenTransferLog::new(v);
        match lg.for_each_nep17(&mut f) {
            Ok(cont) => cont,
            Err(err) => {
                seek_err = Some(err);
                false
            }
        }
    });
    if let Some(err) = seek_err {
        Err(err)
    } else {
        Ok(())
    }
}

// SeekNEP11TransferLog executes f for each NEP-11 transfer in log starting from
// the transfer with the newest timestamp up to the oldest transfer. It continues
// iteration until false is returned from f. The last non-nil error is returned.
pub fn seek_nep11_transfer_log<F>(dao: &Simple, acc: &util::Uint160, newest_timestamp: u64, mut f: F) -> Result<(), Box<dyn Error>>
where
    F: FnMut(&state::NEP11Transfer) -> Result<bool, Box<dyn Error>>,
{
    let key = get_token_transfer_log_key(dao, acc, newest_timestamp, 0, true);
    let prefix_len = 1 + util::UINT160_SIZE;
    let mut seek_err: Option<Box<dyn Error>> = None;
    dao.store.seek(storage::SeekRange {
        prefix: &key[..prefix_len],
        start: &key[prefix_len..prefix_len + 8],
        backwards: true,
    }, |k, v| {
        let mut lg = state::TokenTransferLog::new(v);
        match lg.for_each_nep11(&mut f) {
            Ok(cont) => cont,
            Err(err) => {
                seek_err = Some(err);
                false
            }
        }
    });
    if let Some(err) = seek_err {
        Err(err)
    } else {
        Ok(())
    }
}

// GetTokenTransferLog retrieves transfer log from the cache.
pub fn get_token_transfer_log(dao: &Simple, acc: &util::Uint160, newest_timestamp: u64, index: u32, is_nep11: bool) -> Result<state::TokenTransferLog, Box<dyn Error>> {
    let key = get_token_transfer_log_key(dao, acc, newest_timestamp, index, is_nep11);
    match dao.store.get(&key) {
        Ok(value) => Ok(state::TokenTransferLog::new(&value)),
        Err(e) if e.downcast_ref::<storage::StorageError>() == Some(&storage::StorageError::KeyNotFound) => Ok(state::TokenTransferLog::new_empty()),
        Err(e) => Err(e),
    }
}

// PutTokenTransferLog saves the given transfer log in the cache.
pub fn put_token_transfer_log(dao: &Simple, acc: &util::Uint160, start: u64, index: u32, is_nep11: bool, lg: &state::TokenTransferLog) {
    let key = get_token_transfer_log_key(dao, acc, start, index, is_nep11);
    dao.store.put(&key, lg.raw());
}

// -- end transfer log.

// -- start notification event.

pub fn make_executable_key(dao: &Simple, hash: &util::Uint256) -> Vec<u8> {
    let mut key = dao.get_key_buf(1 + util::UINT256_SIZE);
    key[0] = storage::KeyPrefix::DataExecutable as u8;
    key[1..].copy_from_slice(&hash.bytes_be());
    key
}

// GetAppExecResults gets application execution results with the specified trigger from the
// given store.
pub fn get_app_exec_results(dao: &Simple, hash: &util::Uint256, trig: trigger::Type) -> Result<Vec<state::AppExecResult>, Box<dyn Error>> {
    let key = make_executable_key(dao, hash);
    let bs = dao.store.get(&key)?;
    if bs.is_empty() {
        return Err(Box::new(DaoError::InternalDBInconsistency));
    }
    match bs[0] {
        storage::ExecBlock => {
            let mut r = io::BinReader::new(&bs);
            r.read_u8()?;
            block::new_trimmed_from_reader(dao.version.state_root_in_header, &mut r)?;
            let mut result = Vec::new();
            loop {
                let mut aer = state::AppExecResult::new();
                aer.decode_binary(&mut r)?;
                if r.has_error() {
                    if r.get_error().unwrap().downcast_ref::<std::io::Error>() == Some(&std::io::Error::from(std::io::ErrorKind::UnexpectedEof)) {
                        break;
                    }
                    return Err(Box::new(r.get_error().unwrap()));
                }
                if aer.trigger & trig != 0 {
                    result.push(aer);
                }
            }
            Ok(result)
        }
        storage::ExecTransaction => {
            let (_, _, aer) = decode_tx_and_exec_result(&bs)?;
            if aer.trigger & trig != 0 {
                Ok(vec![aer])
            } else {
                Ok(Vec::new())
            }
        }
        _ => Err(Box::new(DaoError::InternalDBInconsistency)),
    }
}

// GetTxExecResult gets application execution result of the specified transaction
// and returns the transaction itself, its height and its AppExecResult.
pub fn get_tx_exec_result(dao: &Simple, hash: &util::Uint256) -> Result<(u32, transaction::Transaction, state::AppExecResult), Box<dyn Error>> {
    let key = make_executable_key(dao, hash);
    let bs = dao.store.get(&key)?;
    if bs.is_empty() {
        return Err(Box::new(DaoError::InternalDBInconsistency));
    }
    if bs[0] != storage::ExecTransaction {
        return Err(Box::new(storage::StorageError::KeyNotFound));
    }
    decode_tx_and_exec_result(&bs)
}

// decodeTxAndExecResult decodes transaction, its height and execution result from
// the given executable bytes. It performs no executable prefix check.
fn decode_tx_and_exec_result(buf: &[u8]) -> Result<(u32, transaction::Transaction, state::AppExecResult), Box<dyn Error>> {
    if buf.len() == CONFLICT_RECORD_VALUE_LEN {
        return Err(Box::new(storage::StorageError::KeyNotFound));
    }
    let mut r = io::BinReader::new(buf);
    r.read_u8()?;
    let h = r.read_u32_le()?;
    let mut tx = transaction::Transaction::new();
    tx.decode_binary(&mut r)?;
    if r.has_error() {
        return Err(Box::new(r.get_error().unwrap()));
    }
    let mut aer = state::AppExecResult::new();
    aer.decode_binary(&mut r)?;
    if r.has_error() {
        return Err(Box::new(r.get_error().unwrap()));
    }
    Ok((h, tx, aer))
}

// -- end notification event.

// -- start storage item.

// GetStorageItem returns StorageItem if it exists in the given store.
pub fn get_storage_item(dao: &Simple, id: i32, key: &[u8]) -> Option<state::StorageItem> {
    match dao.store.get(&make_storage_item_key(dao, id, key)) {
        Ok(b) => Some(b),
        Err(_) => None,
    }
}

// PutStorageItem puts the given StorageItem for the given id with the given
// key into the given store.
pub fn put_storage_item(dao: &Simple, id: i32, key: &[u8], si: &state::StorageItem) {
    let st_key = make_storage_item_key(dao, id, key);
    dao.store.put(&st_key, si);
}

// PutBigInt serializes and puts the given integer for the given id with the given
// key into the given store.
pub fn put_big_int(dao: &Simple, id: i32, key: &[u8], n: &bigint::BigInt) {
    let mut buf = [0u8; bigint::MAX_BYTES_LEN];
    let st_data = bigint::to_preallocated_bytes(n, &mut buf);
    put_storage_item(dao, id, key, &st_data);
}

// DeleteStorageItem drops a storage item for the given id with the
// given key from the store.
pub fn delete_storage_item(dao: &Simple, id: i32, key: &[u8]) {
    let st_key = make_storage_item_key(dao, id, key);
    dao.store.delete(&st_key);
}

// Seek executes f for all storage items matching the given `rng` (matching the given prefix and
// starting from the point specified). If the key or the value is to be used outside of f, they
// may not be copied. Seek continues iterating until false is returned from f. A requested prefix
// (if any non-empty) is trimmed before passing to f.
pub fn seek<F>(dao: &Simple, id: i32, rng: storage::SeekRange, mut f: F)
where
    F: FnMut(&[u8], &[u8]) -> bool,
{
    let mut rng = rng.clone();
    rng.prefix = make_storage_item_key(dao, id, &rng.prefix);
    dao.store.seek(rng, |k, v| f(&k[rng.prefix.len()..], v));
}

// SeekAsync sends all storage items matching the given `rng` (matching the given prefix and
// starting from the point specified) to a channel and returns the channel.
// Resulting keys and values may not be copied.
pub fn seek_async(dao: &Simple, ctx: &std::sync::Arc<std::sync::Mutex<()>>, id: i32, rng: storage::SeekRange) -> std::sync::mpsc::Receiver<storage::KeyValue> {
    let mut rng = rng.clone();
    rng.prefix = make_storage_item_key(dao, id, &rng.prefix);
    dao.store.seek_async(ctx, rng, true)
}

// makeStorageItemKey returns the key used to store the StorageItem in the DB.
fn make_storage_item_key(dao: &Simple, id: i32, key: &[u8]) -> Vec<u8> {
    let mut buf = dao.get_key_buf(5 + key.len());
    buf[0] = dao.version.storage_prefix as u8;
    buf[1..5].copy_from_slice(&id.to_le_bytes());
    buf[5..].copy_from_slice(key);
    buf
}

// -- end storage item.

// -- other.

// GetBlock returns Block by the given hash if it exists in the store.
pub fn get_block(dao: &Simple, hash: &util::Uint256) -> Result<block::Block, Box<dyn Error>> {
    get_block_with_key(dao, &make_executable_key(dao, hash))
}

fn get_block_with_key(dao: &Simple, key: &[u8]) -> Result<block::Block, Box<dyn Error>> {
    let b = dao.store.get(key)?;
    let mut r = io::BinReader::new(&b);
    if r.read_u8()? != storage::ExecBlock {
        return Err(Box::new(storage::StorageError::KeyNotFound));
    }
    block::new_trimmed_from_reader(dao.version.state_root_in_header, &mut r)
}

// Version represents the current dao version.
#[derive(Clone)]
pub struct Version {
    storage_prefix: storage::KeyPrefix,
    state_root_in_header: bool,
    p2p_sig_extensions: bool,
    p2p_state_exchange_extensions: bool,
    keep_only_latest_state: bool,
    magic: u32,
    value: String,
}

const STATE_ROOT_IN_HEADER_BIT: u8 = 1 << 0;
const P2P_SIG_EXTENSIONS_BIT: u8 = 1 << 1;
const P2P_STATE_EXCHANGE_EXTENSIONS_BIT: u8 = 1 << 2;
const KEEP_ONLY_LATEST_STATE_BIT: u8 = 1 << 3;

// FromBytes decodes v from a byte-slice.
impl Version {
    pub fn from_bytes(data: &[u8]) -> Result<Self, Box<dyn Error>> {
        if data.is_empty() {
            return Err(Box::new(DaoError::InternalDBInconsistency));
        }
        let mut i = 0;
        while i < data.len() && data[i] != 0 {
            i += 1;
        }
        if i == data.len() {
            return Ok(Version {
                value: String::from_utf8(data.to_vec())?,
                storage_prefix: storage::KeyPrefix::STStorage,
                state_root_in_header: false,
                p2p_sig_extensions: false,
                p2p_state_exchange_extensions: false,
                keep_only_latest_state: false,
                magic: 0,
            });
        }
        if data.len() < i + 3 {
            return Err(Box::new(DaoError::InternalDBInconsistency));
        }
        let value = String::from_utf8(data[..i].to_vec())?;
        let storage_prefix = storage::KeyPrefix::from(data[i + 1]);
        let state_root_in_header = data[i + 2] & STATE_ROOT_IN_HEADER_BIT != 0;
        let p2p_sig_extensions = data[i + 2] & P2P_SIG_EXTENSIONS_BIT != 0;
        let p2p_state_exchange_extensions = data[i + 2] & P2P_STATE_EXCHANGE_EXTENSIONS_BIT != 0;
        let keep_only_latest_state = data[i + 2] & KEEP_ONLY_LATEST_STATE_BIT != 0;
        let magic = if data.len() == i + 3 + 4 {
            u32::from_le_bytes(data[i + 3..].try_into()?)
        } else {
            0
        };
        Ok(Version {
            value,
            storage_prefix,
            state_root_in_header,
            p2p_sig_extensions,
            p2p_state_exchange_extensions,
            keep_only_latest_state,
            magic,
        })
    }

    // Bytes encodes v to a byte-slice.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut mask = 0;
        if self.state_root_in_header {
            mask |= STATE_ROOT_IN_HEADER_BIT;
        }
        if self.p2p_sig_extensions {
            mask |= P2P_SIG_EXTENSIONS_BIT;
        }
        if self.p2p_state_exchange_extensions {
            mask |= P2P_STATE_EXCHANGE_EXTENSIONS_BIT;
        }
        if self.keep_only_latest_state {
            mask |= KEEP_ONLY_LATEST_STATE_BIT;
        }
        let mut res = Vec::from(self.value.as_bytes());
        res.push(0);
        res.push(self.storage_prefix as u8);
        res.push(mask);
        res.extend_from_slice(&self.magic.to_le_bytes());
        res
    }
}

pub fn mk_key_prefix(dao: &Simple, k: storage::KeyPrefix) -> Vec<u8> {
    let mut b = dao.get_key_buf(1);
    b[0] = k as u8;
    b
}

// GetVersion attempts to get the current version stored in the
// underlying store.
pub fn get_version(dao: &Simple) -> Result<Version, Box<dyn Error>> {
    let data = dao.store.get(&mk_key_prefix(dao, storage::KeyPrefix::SYSVersion))?;
    Version::from_bytes(&data)
}

// GetCurrentBlockHeight returns the current block height found in the
// underlying store.
pub fn get_current_block_height(dao: &Simple) -> Result<u32, Box<dyn Error>> {
    let b = dao.store.get(&mk_key_prefix(dao, storage::KeyPrefix::SYSCurrentBlock))?;
    Ok(u32::from_le_bytes(b[32..36].try_into()?))
}

// GetCurrentHeaderHeight returns the current header height and hash from
// the underlying store.
pub fn get_current_header_height(dao: &Simple) -> Result<(u32, util::Uint256), Box<dyn Error>> {
    let b = dao.store.get(&mk_key_prefix(dao, storage::KeyPrefix::SYSCurrentHeader))?;
    let i = u32::from_le_bytes(b[32..36].try_into()?);
    let h = util::Uint256::decode_bytes_le(&b[..32])?;
    Ok((i, h))
}

// GetStateSyncPoint returns current state synchronization point P.
pub fn get_state_sync_point(dao: &Simple) -> Result<u32, Box<dyn Error>> {
    let b = dao.store.get(&mk_key_prefix(dao, storage::KeyPrefix::SYSStateSyncPoint))?;
    Ok(u32::from_le_bytes(b.try_into()?))
}

// GetStateSyncCurrentBlockHeight returns the current block height stored during state
// synchronization process.
pub fn get_state_sync_current_block_height(dao: &Simple) -> Result<u32, Box<dyn Error>> {
    let b = dao.store.get(&mk_key_prefix(dao, storage::KeyPrefix::SYSStateSyncCurrentBlockHeight))?;
    Ok(u32::from_le_bytes(b.try_into()?))
}

// GetHeaderHashes returns a page of header hashes retrieved from
// the given underlying store.
pub fn get_header_hashes(dao: &Simple, height: u32) -> Result<Vec<util::Uint256>, Box<dyn Error>> {
    let key = mk_header_hash_key(dao, height);
    let b = dao.store.get(&key)?;
    let mut br = io::BinReader::new(&b);
    let hashes = br.read_array()?;
    if br.has_error() {
        return Err(Box::new(br.get_error().unwrap()));
    }
    Ok(hashes)
}

// DeleteHeaderHashes removes batches of header hashes starting from the one that
// contains header with index `since` up to the most recent batch. It assumes that
// all stored batches contain `batch_size` hashes.
pub fn delete_header_hashes(dao: &Simple, since: u32, batch_size: usize) {
    dao.store.seek(storage::SeekRange {
        prefix: &mk_key_prefix(dao, storage::KeyPrefix::IXHeaderHashList),
        backwards: true,
    }, |k, _| {
        let first = u32::from_be_bytes(k[1..5].try_into().unwrap());
        if first >= since {
            dao.store.delete(k);
            first != since
        } else if first + batch_size as u32 - 1 >= since {
            dao.store.delete(k);
            false
        } else {
            false
        }
    });
}

// GetTransaction returns Transaction and its height by the given hash
// if it exists in the store. It does not return conflict record stubs.
pub fn get_transaction(dao: &Simple, hash: &util::Uint256) -> Result<(transaction::Transaction, u32), Box<dyn Error>> {
    let key = make_executable_key(dao, hash);
    let b = dao.store.get(&key)?;
    if b.len() < 1 {
        return Err(Box::new(DaoError::InternalDBInconsistency));
    }
    if b[0] != storage::ExecTransaction {
        return Err(Box::new(storage::StorageError::KeyNotFound));
    }
    if b.len() == CONFLICT_RECORD_VALUE_LEN {
        return Err(Box::new(storage::StorageError::KeyNotFound));
    }
    let mut r = io::BinReader::new(&b);
    r.read_u8()?;
    let height = r.read_u32_le()?;
    let mut tx = transaction::Transaction::new();
    tx.decode_binary(&mut r)?;
    if r.has_error() {
        return Err(Box::new(r.get_error().unwrap()));
    }
    Ok((tx, height))
}

// PutVersion stores the given version in the underlying store.
pub fn put_version(dao: &Simple, v: Version) {
    dao.version = v.clone();
    dao.store.put(&mk_key_prefix(dao, storage::KeyPrefix::SYSVersion), &v.to_bytes());
}

// PutCurrentHeader stores the current header.
pub fn put_current_header(dao: &Simple, h: &util::Uint256, index: u32) {
    let mut buf = dao.get_data_buf();
    buf.write_bytes(&h.bytes_le());
    buf.write_u32_le(index);
    dao.store.put(&mk_key_prefix(dao, storage::KeyPrefix::SYSCurrentHeader), buf.bytes());
}

// PutStateSyncPoint stores the current state synchronization point P.
pub fn put_state_sync_point(dao: &Simple, p: u32) {
    let mut buf = dao.get_data_buf();
    buf.write_u32_le(p);
    dao.store.put(&mk_key_prefix(dao, storage::KeyPrefix::SYSStateSyncPoint), buf.bytes());
}

// PutStateSyncCurrentBlockHeight stores the current block height during state synchronization process.
pub fn put_state_sync_current_block_height(dao: &Simple, h: u32) {
    let mut buf = dao.get_data_buf();
    buf.write_u32_le(h);
    dao.store.put(&mk_key_prefix(dao, storage::KeyPrefix::SYSStateSyncCurrentBlockHeight), buf.bytes());
}

fn mk_header_hash_key(dao: &Simple, h: u32) -> Vec<u8> {
    let mut b = dao.get_key_buf(1 + 4);
    b[0] = storage::KeyPrefix::IXHeaderHashList as u8;
    b[1..5].copy_from_slice(&h.to_be_bytes());
    b
}

// StoreHeaderHashes pushes a batch of header hashes into the store.
pub fn store_header_hashes(dao: &Simple, hashes: &[util::Uint256], height: u32) -> Result<(), Box<dyn Error>> {
    let key = mk_header_hash_key(dao, height);
    let mut buf = dao.get_data_buf();
    buf.write_array(hashes)?;
    if buf.has_error() {
        return Err(Box::new(buf.get_error().unwrap()));
    }
    dao.store.put(&key, buf.bytes());
    Ok(())
}

// HasTransaction returns nil if the given store does not contain the given
// Transaction hash. It returns an error in case the transaction is in chain
// or in the list of conflicting transactions. If non-zero signers are specified,
// then additional check against the conflicting transaction signers intersection
// is held. Do not omit signers in case if it's important to check the validity
// of a supposedly conflicting on-chain transaction. The retrieved conflict isn't
// checked against the maxTraceableBlocks setting if signers are omitted.
// HasTransaction does not consider the case of block executable.
pub fn has_transaction(dao: &Simple, hash: &util::Uint256, signers: &[transaction::Signer], current_index: u32, max_traceable_blocks: u32) -> Result<(), Box<dyn Error>> {
    let key = make_executable_key(dao, hash);
    let bytes = match dao.store.get(&key) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(()),
    };
    if bytes.len() < CONFLICT_RECORD_VALUE_LEN {
        return Ok(());
    }
    if bytes[0] != storage::ExecTransaction {
        return Ok(());
    }
    if bytes.len() != CONFLICT_RECORD_VALUE_LEN {
        return Err(Box::new(DaoError::AlreadyExists));
    }
    if signers.is_empty() {
        return Err(Box::new(DaoError::HasConflicts));
    }
    if !is_traceable_block(&bytes[1..], current_index, max_traceable_blocks) {
        return Ok(());
    }
    for s in signers {
        let v = match dao.store.get(&[key.clone(), s.account.bytes_be()].concat()) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if is_traceable_block(&v[1..], current_index, max_traceable_blocks) {
            return Err(Box::new(DaoError::HasConflicts));
        }
    }
    Ok(())
}

fn is_traceable_block(index_bytes: &[u8], height: u32, max_traceable_blocks: u32) -> bool {
    let index = u32::from_le_bytes(index_bytes.try_into().unwrap());
    index <= height && index + max_traceable_blocks > height
}

// StoreAsBlock stores given block as DataBlock. It can reuse given buffer for
// the purpose of value serialization.
pub fn store_as_block(dao: &Simple, block: &block::Block, aer1: Option<&state::AppExecResult>, aer2: Option<&state::AppExecResult>) -> Result<(), Box<dyn Error>> {
    let key = make_executable_key(dao, &block.hash());
    let mut buf = dao.get_data_buf();
    buf.write_u8(storage::ExecBlock as u8);
    block.encode_trimmed(&mut buf)?;
    if let Some(aer1) = aer1 {
        aer1.encode_binary_with_context(&mut buf, &dao.get_item_ctx());
    }
    if let Some(aer2) = aer2 {
        aer2.encode_binary_with_context(&mut buf, &dao.get_item_ctx());
    }
    if buf.has_error() {
        return Err(Box::new(buf.get_error().unwrap()));
    }
    dao.store.put(&key, buf.bytes());
    Ok(())
}

// DeleteBlock removes the block from dao. It's not atomic, so make sure you're
// using private MemCached instance here.
pub fn delete_block(dao: &Simple, h: &util::Uint256) -> Result<(), Box<dyn Error>> {
    let key = make_executable_key(dao, h);
    let b = get_block_with_key(dao, &key)?;
    store_header(dao, &key, &b.header)?;
    for tx in &b.transactions {
        let mut key = key.clone();
        key[1..].copy_from_slice(&tx.hash().bytes_be());
        dao.store.delete(&key);
        for attr in tx.get_attributes(transaction::ConflictsT) {
            let hash = attr.value().as_conflicts().hash();
            key[1..].copy_from_slice(&hash.bytes_be());
            let v = dao.store.get(&key)?;
            if v[0] != storage::ExecTransaction {
                continue;
            }
            let index = u32::from_le_bytes(v[1..5].try_into().unwrap());
            if index == b.index {
                dao.store.delete(&key);
            }
            for s in &tx.signers {
                let s_key = [key.clone(), s.account.bytes_be()].concat();
                let v = dao.store.get(&s_key)?;
                let index = u32::from_le_bytes(v[1..5].try_into().unwrap());
                if index == b.index {
                    dao.store.delete(&s_key);
                }
            }
        }
    }
    Ok(())
}

// PurgeHeader completely removes specified header from dao. It differs from
// DeleteBlock in that it removes header anyway and does nothing except removing
// header. It does no checks for header existence.
pub fn purge_header(dao: &Simple, h: &util::Uint256) {
    let key = make_executable_key(dao, h);
    dao.store.delete(&key);
}

// StoreHeader saves the block header into the store.
pub fn store_header(dao: &Simple, key: &[u8], h: &block::Header) -> Result<(), Box<dyn Error>> {
    let mut buf = dao.get_data_buf();
    buf.write_u8(storage::ExecBlock as u8);
    h.encode_binary(&mut buf)?;
    buf.write_u8(0);
    if buf.has_error() {
        return Err(Box::new(buf.get_error().unwrap()));
    }
    dao.store.put(key, buf.bytes());
    Ok(())
}

// StoreAsCurrentBlock stores the hash of the given block with prefix
// SYSCurrentBlock.
pub fn store_as_current_block(dao: &Simple, block: &block::Block) {
    let mut buf = dao.get_data_buf();
    let h = block.hash();
    h.encode_binary(&mut buf);
    buf.write_u32_le(block.index);
    dao.store.put(&mk_key_prefix(dao, storage::KeyPrefix::SYSCurrentBlock), buf.bytes());
}

// StoreAsTransaction stores the given TX as DataTransaction. It also stores conflict records
// (hashes of transactions the given tx has conflicts with) as DataTransaction with value containing
// only five bytes: 1-byte [storage.ExecTransaction] executable prefix + 4-bytes-LE block index. It can reuse the given
// buffer for the purpose of value serialization.
pub fn store_as_transaction(dao: &Simple, tx: &transaction::Transaction, index: u32, aer: Option<&state::AppExecResult>) -> Result<(), Box<dyn Error>> {
    let key = make_executable_key(dao, &tx.hash());
    let mut buf = dao.get_data_buf();
    buf.write_u8(storage::ExecTransaction as u8);
    buf.write_u32_le(index);
    tx.encode_binary(&mut buf)?;
    if let Some(aer) = aer {
        aer.encode_binary_with_context(&mut buf, &dao.get_item_ctx());
    }
    if buf.has_error() {
        return Err(Box::new(buf.get_error().unwrap()));
    }
    let val = buf.bytes().to_vec();
    dao.store.put(&key, &val);
    let val = &val[..CONFLICT_RECORD_VALUE_LEN];
    for attr in tx.get_attributes(transaction::ConflictsT) {
        let hash = attr.value().as_conflicts().hash();
        let mut key = key.clone();
        key[1..].copy_from_slice(&hash.bytes_be());
        if let Ok(exec) = dao.store.get(&key) {
            if !exec.is_empty() && exec[0] != storage::ExecTransaction {
                continue;
            }
        }
        dao.store.put(&key, val);
        let s_key = [key.clone(), vec![0; util::UINT160_SIZE]].concat();
        for s in &tx.signers {
            let mut s_key = s_key.clone();
            s_key[key.len()..].copy_from_slice(&s.account.bytes_be());
            dao.store.put(&s_key, val);
        }
    }
    Ok(())
}

impl Simple {
    fn get_key_buf(&self, l: usize) -> Vec<u8> {
        if self.private.load(Ordering::Relaxed) {
            let mut key_buf = self.key_buf.lock().unwrap();
            if key_buf.is_empty() {
                *key_buf = vec![0; 1 + 4 + limits::MAX_STORAGE_KEY_LEN];
            }
            key_buf[..l].to_vec()
        } else {
            vec![0; l]
        }
    }

    fn get_data_buf(&self) -> io::BufBinWriter {
        if self.private.load(Ordering::Relaxed) {
            let mut data_buf = self.data_buf.lock().unwrap();
            data_buf.reset();
            data_buf.clone()
        } else {
            io::BufBinWriter::new()
        }
    }

    fn get_item_ctx(&self) -> stackitem::SerializationContext {
        if self.private.load(Ordering::Relaxed) {
            let mut ser_ctx = self.ser_ctx.lock().unwrap();
            if ser_ctx.is_none() {
                *ser_ctx = Some(stackitem::SerializationContext::new());
            }
            ser_ctx.clone().unwrap()
        } else {
            stackitem::SerializationContext::new()
        }
    }

    // Persist flushes all the changes made into the (supposedly) persistent
    // underlying store. It doesn't block accesses to DAO from other threads.
    pub fn persist(&self) -> Result<i32, Box<dyn Error>> {
        if let Some(ref native_cache_ps) = self.native_cache_ps {
            let _lock1 = self.native_cache_lock.write().unwrap();
            let _lock2 = native_cache_ps.native_cache_lock.write().unwrap();
            self.persist_native_cache();
        }
        self.store.persist()
    }

    // PersistSync flushes all the changes made into the (supposedly) persistent
    // underlying store. It's a synchronous version of Persist that doesn't allow
    // other threads to work with DAO while flushing the Store.
    pub fn persist_sync(&self) -> Result<i32, Box<dyn Error>> {
        if let Some(ref native_cache_ps) = self.native_cache_ps {
            let _lock1 = self.native_cache_lock.write().unwrap();
            let _lock2 = native_cache_ps.native_cache_lock.write().unwrap();
            self.persist_native_cache();
        }
        self.store.persist_sync()
    }

    // persistNativeCache is internal unprotected method for native cache persisting.
    // It does NO checks for nativeCachePS is not nil.
    fn persist_native_cache(&self) {
        if let Some(ref lower) = self.native_cache_ps {
            for (id, native_cache) in &self.native_cache {
                lower.native_cache.insert(*id, native_cache.copy());
            }
        }
        self.native_cache.clear();
    }

    // GetROCache returns native contact cache. The cache CAN NOT be modified by
    // the caller. It's the caller's duty to keep it unmodified.
    pub fn get_ro_cache(&self, id: i32) -> Option<Box<dyn NativeContractCache>> {
        let _lock = self.native_cache_lock.read().unwrap();
        self.get_cache(id, true)
    }

    // GetRWCache returns native contact cache. The cache CAN BE safely modified
    // by the caller.
    pub fn get_rw_cache(&self, id: i32) -> Option<Box<dyn NativeContractCache>> {
        let _lock = self.native_cache_lock.write().unwrap();
        self.get_cache(id, false)
    }

    // getCache is an internal unlocked representation of GetROCache and GetRWCache.
    fn get_cache(&self, k: i32, ro: bool) -> Option<Box<dyn NativeContractCache>> {
        if let Some(itm) = self.native_cache.get(&k) {
            return Some(itm.copy());
        }
        if let Some(ref native_cache_ps) = self.native_cache_ps {
            if ro {
                return native_cache_ps.get_ro_cache(k);
            }
            if let Some(v) = native_cache_ps.get_rw_cache(k) {
                let cp = v.copy();
                self.native_cache.insert(k, cp.copy());
                return Some(cp);
            }
        }
        None
    }

    // SetCache adds native contract cache to the cache map.
    pub fn set_cache(&self, id: i32, v: Box<dyn NativeContractCache>) {
        let _lock = self.native_cache_lock.write().unwrap();
        self.native_cache.insert(id, v);
    }
}
