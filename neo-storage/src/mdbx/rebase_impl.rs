//! Offline, verified MDBX environment rebasing.
//!
//! Rebasing copies logical rows into a fresh environment. It is deliberately
//! offline and fail-closed: the source is opened exclusively, the destination
//! must not exist, and an incomplete sentinel prevents normal node startup
//! until every copied table has been verified.

#![allow(unsafe_code)]

use super::support::{
    create_incomplete_destination, database_file_bytes, encode_hex, hash_entry,
    hash_length_prefixed, is_environment_id, mdbx_error, raw_table_flags, read_environment_id,
    remove_incomplete_sentinel, table_label,
};
use crate::{StorageError, StorageResult};
use libmdbx::{
    Database, DatabaseOptions, Error as MdbxError, Mode, NoWriteMap, ReadWriteOptions, SyncMode,
    TableFlags, WriteFlags,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    borrow::Cow,
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
use tracing::info;

const DEFAULT_MAX_TABLES: u64 = 64;
const DEFAULT_MAP_SIZE: isize = 512 * 1024 * 1024 * 1024;
const DEFAULT_GROWTH_STEP: isize = 256 * 1024 * 1024;
const DEFAULT_MAX_READERS: u32 = 64;
const DEFAULT_BATCH_SCANNED_ROWS: u64 = 1_000_000;
const DEFAULT_BATCH_RETAINED_BYTES: usize = 256 * 1024 * 1024;
const DIGEST_DOMAIN: &[u8] = b"neo-mdbx-rebase-ordered-table-v1";
const ENVIRONMENT_ID_DIGEST_PLACEHOLDER: &[u8] = b"regenerated-environment-id-v1";

/// A sentinel whose presence makes the normal MDBX store reject an environment.
pub const MDBX_REBASE_INCOMPLETE_FILE: &str = ".neo-mdbx-rebase-incomplete";

/// One exact key-shape exclusion applied to a named table during rebase.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MdbxExactKeyExclusion {
    /// Named table containing the obsolete rows.
    pub table: String,
    /// Required key prefix.
    pub prefix: Vec<u8>,
    /// Required complete key length.
    pub key_length: usize,
}

impl MdbxExactKeyExclusion {
    /// Creates an exact key-shape exclusion.
    pub fn new(table: impl Into<String>, prefix: Vec<u8>, key_length: usize) -> Self {
        Self {
            table: table.into(),
            prefix,
            key_length,
        }
    }

    fn matches(&self, table: Option<&str>, key: &[u8]) -> bool {
        table == Some(self.table.as_str())
            && key.len() == self.key_length
            && key.starts_with(&self.prefix)
    }
}

/// Bounded options for one offline MDBX rebase.
#[derive(Clone, Debug)]
pub struct MdbxRebaseOptions {
    /// Existing source environment, opened read-only and exclusively.
    pub source: PathBuf,
    /// Fresh destination directory. It must not already exist.
    pub destination: PathBuf,
    /// Exact expected named-table catalog. Unknown or missing tables abort.
    pub expected_named_tables: Vec<String>,
    /// Exact obsolete key shape to omit.
    pub exclusion: MdbxExactKeyExclusion,
    /// Destination MDBX upper geometry.
    pub geometry_upper_bytes: isize,
    /// Destination MDBX growth step.
    pub geometry_growth_bytes: isize,
    /// Requested MDBX reader limit.
    pub max_readers: u32,
    /// Maximum source rows inspected in one frozen transaction.
    pub batch_scanned_rows: u64,
    /// Maximum retained key/value bytes buffered for one destination commit.
    pub batch_retained_bytes: usize,
}

impl MdbxRebaseOptions {
    /// Creates options with durable, bounded production defaults.
    pub fn new(
        source: impl Into<PathBuf>,
        destination: impl Into<PathBuf>,
        expected_named_tables: Vec<String>,
        exclusion: MdbxExactKeyExclusion,
    ) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            expected_named_tables,
            exclusion,
            geometry_upper_bytes: DEFAULT_MAP_SIZE,
            geometry_growth_bytes: DEFAULT_GROWTH_STEP,
            max_readers: DEFAULT_MAX_READERS,
            batch_scanned_rows: DEFAULT_BATCH_SCANNED_ROWS,
            batch_retained_bytes: DEFAULT_BATCH_RETAINED_BYTES,
        }
    }
}

/// Verified logical-copy evidence for one MDBX table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MdbxRebaseTableReport {
    /// Main or named table.
    pub table: String,
    /// Raw source rows, including Main DB table descriptors.
    pub source_rows: u64,
    /// Raw destination rows, including recreated Main DB descriptors.
    pub destination_rows: u64,
    /// Logical rows copied and digest-verified.
    pub copied_rows: u64,
    /// Rows intentionally excluded by the exact key predicate.
    pub excluded_rows: u64,
    /// Main DB named-table descriptors recreated by MDBX rather than copied.
    pub descriptor_rows: u64,
    /// Retained key bytes.
    pub copied_key_bytes: u64,
    /// Retained value bytes.
    pub copied_value_bytes: u64,
    /// Excluded key bytes.
    pub excluded_key_bytes: u64,
    /// Excluded value bytes.
    pub excluded_value_bytes: u64,
    /// Source table flags, preserved exactly.
    pub table_flags: u32,
    /// Domain-separated ordered SHA-256 of every retained key/value row.
    pub ordered_sha256: String,
}

/// Complete evidence emitted after a successful MDBX rebase.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MdbxRebaseReport {
    /// Source path.
    pub source: PathBuf,
    /// Destination path.
    pub destination: PathBuf,
    /// Frozen source transaction ID.
    pub source_transaction_id: u64,
    /// Source environment identity, encoded as lowercase hexadecimal.
    pub source_environment_id: String,
    /// Fresh destination environment identity, encoded as lowercase hexadecimal.
    pub destination_environment_id: String,
    /// Exact discovered named-table catalog.
    pub named_tables: Vec<String>,
    /// Applied exclusion.
    pub exclusion: MdbxExactKeyExclusion,
    /// Per-table copy and verification evidence.
    pub tables: Vec<MdbxRebaseTableReport>,
    /// Source mdbx.dat size before the copy.
    pub source_file_bytes: u64,
    /// Destination mdbx.dat size after forced synchronization.
    pub destination_file_bytes: u64,
    /// End-to-end elapsed wall time.
    pub elapsed_ms: u64,
}

#[derive(Default)]
struct LogicalStats {
    raw_rows: u64,
    logical_rows: u64,
    descriptor_rows: u64,
    excluded_rows: u64,
    key_bytes: u64,
    value_bytes: u64,
    excluded_key_bytes: u64,
    excluded_value_bytes: u64,
}

struct TableCopy {
    stats: LogicalStats,
    digest: Sha256,
}

impl TableCopy {
    fn new(table: Option<&str>) -> Self {
        let mut digest = Sha256::new();
        digest.update(DIGEST_DOMAIN);
        hash_length_prefixed(&mut digest, table_label(table).as_bytes());
        Self {
            stats: LogicalStats::default(),
            digest,
        }
    }
}

struct RetainedRow {
    key: Vec<u8>,
    value: Vec<u8>,
}

struct ScanBatch {
    rows: Vec<RetainedRow>,
    last_key: Option<Vec<u8>>,
    finished: bool,
    scanned: u64,
}

/// Copies, verifies, and durably closes a fresh MDBX environment.
///
/// The source is never modified. Any failure after destination creation leaves
/// the incomplete sentinel in place, so normal node composition rejects it.
/// Success also retains the sentinel until the caller durably publishes the
/// returned evidence and calls finalize_mdbx_rebase.
pub fn rebase_mdbx_environment(options: &MdbxRebaseOptions) -> StorageResult<MdbxRebaseReport> {
    validate_options(options)?;
    let started = Instant::now();
    let source = open_source(options)?;
    let source_info = source
        .info()
        .map_err(|error| mdbx_error("read source info", error))?;
    let source_environment_id = read_environment_id(&source)?;
    let mut destination_environment_id = rand::random::<[u8; 16]>();
    if destination_environment_id == source_environment_id {
        destination_environment_id[0] ^= 1;
    }
    let named_tables = discover_named_tables(&source)?;
    let mut expected = options.expected_named_tables.clone();
    expected.sort();
    expected.dedup();
    if named_tables != expected {
        return Err(StorageError::invalid_operation(format!(
            "MDBX named-table catalog differs from the required catalog: discovered={named_tables:?}, expected={expected:?}"
        )));
    }
    if !named_tables.contains(&options.exclusion.table) {
        return Err(StorageError::invalid_operation(format!(
            "MDBX exclusion table {:?} is absent from the verified catalog",
            options.exclusion.table
        )));
    }

    let table_flags = read_table_flags(&source, &named_tables)?;
    let descriptor_keys = named_tables
        .iter()
        .map(|name| name.as_bytes().to_vec())
        .collect::<BTreeSet<_>>();
    create_incomplete_destination(&options.destination)?;
    let destination = open_destination(options)?;

    let mut copied = Vec::with_capacity(named_tables.len() + 1);
    copied.push(copy_table(
        &source,
        &destination,
        None,
        table_flags[0],
        &descriptor_keys,
        &destination_environment_id,
        options,
    )?);
    create_named_tables(&destination, &named_tables, &table_flags[1..])?;
    for (index, name) in named_tables.iter().enumerate() {
        copied.push(copy_table(
            &source,
            &destination,
            Some(name),
            table_flags[index + 1],
            &descriptor_keys,
            &destination_environment_id,
            options,
        )?);
    }

    destination
        .sync(true)
        .map_err(|error| mdbx_error("force destination sync", error))?;
    let reports = verify_destination(
        &destination,
        &named_tables,
        &table_flags,
        &descriptor_keys,
        &options.exclusion,
        copied,
    )?;
    let verified_destination_environment_id = read_environment_id(&destination)?;
    if verified_destination_environment_id != destination_environment_id {
        return Err(StorageError::backend(
            "MDBX rebase destination environment identity changed during verification",
        ));
    }
    destination
        .sync(true)
        .map_err(|error| mdbx_error("force verified destination sync", error))?;
    drop(destination);
    drop(source);

    let source_file_bytes = database_file_bytes(&options.source)?;
    let destination_file_bytes = database_file_bytes(&options.destination)?;
    Ok(MdbxRebaseReport {
        source: options.source.clone(),
        destination: options.destination.clone(),
        source_transaction_id: source_info.last_txnid().try_into().unwrap_or(u64::MAX),
        source_environment_id: encode_hex(&source_environment_id),
        destination_environment_id: encode_hex(&destination_environment_id),
        named_tables,
        exclusion: options.exclusion.clone(),
        tables: reports,
        source_file_bytes,
        destination_file_bytes,
        elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}

fn validate_options(options: &MdbxRebaseOptions) -> StorageResult<()> {
    if options.source.join(MDBX_REBASE_INCOMPLETE_FILE).exists() {
        return Err(StorageError::invalid_operation(format!(
            "MDBX source {} is marked as an incomplete rebase",
            options.source.display()
        )));
    }
    if !options.source.join("mdbx.dat").is_file() {
        return Err(StorageError::invalid_operation(format!(
            "MDBX source {} has no mdbx.dat",
            options.source.display()
        )));
    }
    if options.destination.exists() {
        return Err(StorageError::invalid_operation(format!(
            "MDBX rebase destination {} already exists",
            options.destination.display()
        )));
    }
    let source = fs::canonicalize(&options.source).map_err(|error| {
        StorageError::io(format!(
            "canonicalize MDBX rebase source {}: {error}",
            options.source.display()
        ))
    })?;
    let destination = normalize_new_path(&options.destination)?;
    if destination.starts_with(&source) || source.starts_with(&destination) {
        return Err(StorageError::invalid_operation(format!(
            "MDBX rebase source {} and destination {} must not contain one another",
            source.display(),
            destination.display()
        )));
    }
    if options.expected_named_tables.is_empty()
        || options.expected_named_tables.iter().any(String::is_empty)
    {
        return Err(StorageError::invalid_operation(
            "MDBX rebase requires a non-empty named-table catalog",
        ));
    }
    if options.exclusion.prefix.is_empty()
        || options.exclusion.key_length < options.exclusion.prefix.len()
    {
        return Err(StorageError::invalid_operation(
            "MDBX rebase exclusion must have a non-empty prefix within its exact key length",
        ));
    }
    if options.geometry_upper_bytes <= 0
        || options.geometry_growth_bytes <= 0
        || options.batch_scanned_rows == 0
        || options.batch_retained_bytes == 0
    {
        return Err(StorageError::invalid_operation(
            "MDBX rebase geometry and batch bounds must be positive",
        ));
    }
    Ok(())
}

/// Publishes a verified rebase after its external evidence report is durable.
pub fn finalize_mdbx_rebase(destination: &Path) -> StorageResult<()> {
    if !destination.join("mdbx.dat").is_file() {
        return Err(StorageError::invalid_operation(format!(
            "MDBX rebase destination {} has no mdbx.dat",
            destination.display()
        )));
    }
    if !destination.join(MDBX_REBASE_INCOMPLETE_FILE).is_file() {
        return Err(StorageError::invalid_operation(format!(
            "MDBX rebase destination {} is not awaiting finalization",
            destination.display()
        )));
    }
    remove_incomplete_sentinel(destination)
}

fn normalize_new_path(path: &Path) -> StorageResult<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|error| {
        StorageError::io(format!(
            "canonicalize parent {} for new path {}: {error}",
            parent.display(),
            path.display()
        ))
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        StorageError::invalid_operation(format!("path {} has no final component", path.display()))
    })?;
    Ok(parent.join(file_name))
}

fn open_source(options: &MdbxRebaseOptions) -> StorageResult<Database<NoWriteMap>> {
    let database = Database::open_with_options(
        &options.source,
        DatabaseOptions {
            max_readers: Some(options.max_readers),
            max_tables: Some(DEFAULT_MAX_TABLES),
            mode: Mode::ReadOnly,
            exclusive: true,
            ..Default::default()
        },
    )
    .map_err(|error| mdbx_error("open source read-only and exclusive", error))?;
    // MDBX normally disables kernel readahead when an environment exceeds
    // physical RAM. Rebase is a verified full ordered scan, so explicitly
    // restore normal readahead without touching the database contents.
    let transaction = database
        .begin_ro_txn()
        .map_err(|error| mdbx_error("begin source readahead hint transaction", error))?;
    // SAFETY: the transaction owns a live MDBX environment pointer for this
    // call, and the warmup API only updates MDBX's shared readahead hint.
    let result = unsafe {
        mdbx_sys::mdbx_env_warmup(
            std::ptr::null(),
            transaction.txn().0,
            mdbx_sys::MDBX_warmup_default,
            0,
        )
    };
    drop(transaction);
    if result != mdbx_sys::MDBX_SUCCESS && result != mdbx_sys::MDBX_RESULT_TRUE {
        return Err(mdbx_error(
            "enable source sequential readahead",
            MdbxError::from_err_code(result),
        ));
    }
    Ok(database)
}

fn open_destination(options: &MdbxRebaseOptions) -> StorageResult<Database<NoWriteMap>> {
    Database::open_with_options(
        &options.destination,
        DatabaseOptions {
            max_readers: Some(options.max_readers),
            max_tables: Some(DEFAULT_MAX_TABLES),
            exclusive: true,
            mode: Mode::ReadWrite(ReadWriteOptions {
                sync_mode: SyncMode::Durable,
                max_size: Some(options.geometry_upper_bytes),
                growth_step: Some(options.geometry_growth_bytes),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .map_err(|error| mdbx_error("open fresh destination", error))
}

fn discover_named_tables(db: &Database<NoWriteMap>) -> StorageResult<Vec<String>> {
    let transaction = db
        .begin_ro_txn()
        .map_err(|error| mdbx_error("begin table-catalog transaction", error))?;
    let main = transaction
        .open_table(None)
        .map_err(|error| mdbx_error("open Main DB for table discovery", error))?;
    let mut cursor = transaction
        .cursor(&main)
        .map_err(|error| mdbx_error("open Main DB catalog cursor", error))?;
    let mut entry = cursor
        .first::<Cow<'_, [u8]>, ()>()
        .map_err(|error| mdbx_error("read first Main DB catalog key", error))?;
    let mut names = BTreeSet::new();
    while let Some((key, ())) = entry {
        if !key.is_empty()
            && !key.contains(&0)
            && let Ok(name) = std::str::from_utf8(key.as_ref())
        {
            match transaction.open_table(Some(name)) {
                Ok(_) => {
                    names.insert(name.to_owned());
                }
                Err(MdbxError::Incompatible | MdbxError::NotFound | MdbxError::BadValSize) => {}
                Err(error) => {
                    return Err(mdbx_error(
                        &format!("probe Main DB key {name:?} as a named table"),
                        error,
                    ));
                }
            }
        }
        entry = cursor
            .next::<Cow<'_, [u8]>, ()>()
            .map_err(|error| mdbx_error("advance Main DB catalog cursor", error))?;
    }
    Ok(names.into_iter().collect())
}

fn read_table_flags(
    db: &Database<NoWriteMap>,
    named_tables: &[String],
) -> StorageResult<Vec<TableFlags>> {
    let transaction = db
        .begin_ro_txn()
        .map_err(|error| mdbx_error("begin table-flags transaction", error))?;
    std::iter::once(None)
        .chain(named_tables.iter().map(|name| Some(name.as_str())))
        .map(|name| {
            let table = transaction
                .open_table(name)
                .map_err(|error| mdbx_error("open source table for flags", error))?;
            let flags = raw_table_flags(&transaction, &table, name)?;
            if flags.contains(TableFlags::DUP_SORT) {
                return Err(StorageError::invalid_operation(format!(
                    "MDBX rebase does not support duplicate-sorted table {:?}",
                    table_label(name)
                )));
            }
            Ok(flags)
        })
        .collect()
}

fn create_named_tables(
    destination: &Database<NoWriteMap>,
    names: &[String],
    flags: &[TableFlags],
) -> StorageResult<()> {
    let transaction = destination
        .begin_rw_txn()
        .map_err(|error| mdbx_error("begin destination table creation", error))?;
    for (name, flags) in names.iter().zip(flags.iter().copied()) {
        transaction
            .create_table(Some(name), flags)
            .map_err(|error| mdbx_error(&format!("create destination table {name:?}"), error))?;
    }
    transaction
        .commit()
        .map_err(|error| mdbx_error("commit destination table creation", error))?;
    Ok(())
}

fn copy_table(
    source: &Database<NoWriteMap>,
    destination: &Database<NoWriteMap>,
    table_name: Option<&str>,
    table_flags: TableFlags,
    descriptor_keys: &BTreeSet<Vec<u8>>,
    destination_environment_id: &[u8; 16],
    options: &MdbxRebaseOptions,
) -> StorageResult<TableCopy> {
    let mut copy = TableCopy::new(table_name);
    let mut last_key = None;
    let mut batches = 0u64;
    loop {
        let batch = scan_batch(
            source,
            table_name,
            last_key.as_deref(),
            descriptor_keys,
            &options.exclusion,
            destination_environment_id,
            options.batch_scanned_rows,
            options.batch_retained_bytes,
            &mut copy,
        )?;
        if !batch.rows.is_empty() {
            append_batch(destination, table_name, table_flags, &batch.rows)?;
        }
        batches = batches.saturating_add(1);
        if batch.finished || batches.is_multiple_of(16) || !batch.rows.is_empty() {
            info!(
                table = table_label(table_name),
                batches,
                batch_scanned = batch.scanned,
                source_rows = copy.stats.raw_rows,
                copied_rows = copy.stats.logical_rows,
                excluded_rows = copy.stats.excluded_rows,
                "MDBX rebase table progress"
            );
        }
        last_key = batch.last_key;
        if batch.finished {
            break;
        }
    }
    Ok(copy)
}

#[allow(clippy::too_many_arguments)]
fn scan_batch(
    source: &Database<NoWriteMap>,
    table_name: Option<&str>,
    last_key: Option<&[u8]>,
    descriptor_keys: &BTreeSet<Vec<u8>>,
    exclusion: &MdbxExactKeyExclusion,
    destination_environment_id: &[u8; 16],
    maximum_scanned_rows: u64,
    maximum_retained_bytes: usize,
    copy: &mut TableCopy,
) -> StorageResult<ScanBatch> {
    let transaction = source
        .begin_ro_txn()
        .map_err(|error| mdbx_error("begin source scan transaction", error))?;
    let table = transaction
        .open_table(table_name)
        .map_err(|error| mdbx_error("open source scan table", error))?;
    let mut cursor = transaction
        .cursor(&table)
        .map_err(|error| mdbx_error("open source scan cursor", error))?;
    let mut entry = match last_key {
        Some(last_key) => {
            let found = cursor
                .set_range::<Cow<'_, [u8]>, Cow<'_, [u8]>>(last_key)
                .map_err(|error| mdbx_error("resume source scan cursor", error))?;
            match found {
                Some((key, _)) if key.as_ref() == last_key => cursor
                    .next::<Cow<'_, [u8]>, Cow<'_, [u8]>>()
                    .map_err(|error| mdbx_error("advance resumed source scan cursor", error))?,
                other => other,
            }
        }
        None => cursor
            .first::<Cow<'_, [u8]>, Cow<'_, [u8]>>()
            .map_err(|error| mdbx_error("read first source row", error))?,
    };
    let mut rows = Vec::new();
    let mut retained_bytes = 0usize;
    let mut scanned = 0u64;
    let mut batch_last_key = last_key.map_or_else(Vec::new, ToOwned::to_owned);
    let mut saw_row = false;
    let mut finished = false;
    loop {
        let Some((key, value)) = entry else {
            finished = true;
            break;
        };
        let key = key.as_ref();
        let value = value.as_ref();
        scanned = scanned.saturating_add(1);
        copy.stats.raw_rows = copy.stats.raw_rows.saturating_add(1);
        batch_last_key.clear();
        batch_last_key.extend_from_slice(key);
        saw_row = true;
        if table_name.is_none() && descriptor_keys.contains(key) {
            copy.stats.descriptor_rows = copy.stats.descriptor_rows.saturating_add(1);
        } else if exclusion.matches(table_name, key) {
            copy.stats.excluded_rows = copy.stats.excluded_rows.saturating_add(1);
            copy.stats.excluded_key_bytes = copy
                .stats
                .excluded_key_bytes
                .saturating_add(key.len() as u64);
            copy.stats.excluded_value_bytes = copy
                .stats
                .excluded_value_bytes
                .saturating_add(value.len() as u64);
        } else {
            if is_environment_id(table_name, key) && value.len() != 16 {
                return Err(StorageError::invalid_data(format!(
                    "source MDBX environment identity has invalid length {}",
                    value.len()
                )));
            }
            copy.stats.logical_rows = copy.stats.logical_rows.saturating_add(1);
            copy.stats.key_bytes = copy.stats.key_bytes.saturating_add(key.len() as u64);
            copy.stats.value_bytes = copy.stats.value_bytes.saturating_add(value.len() as u64);
            let digest_value = if is_environment_id(table_name, key) {
                ENVIRONMENT_ID_DIGEST_PLACEHOLDER
            } else {
                value
            };
            hash_entry(&mut copy.digest, key, digest_value);
            retained_bytes = retained_bytes
                .saturating_add(key.len())
                .saturating_add(value.len());
            rows.push(RetainedRow {
                key: key.to_vec(),
                value: if is_environment_id(table_name, key) {
                    destination_environment_id.to_vec()
                } else {
                    value.to_vec()
                },
            });
        }
        if scanned >= maximum_scanned_rows || retained_bytes >= maximum_retained_bytes {
            break;
        }
        entry = cursor
            .next::<Cow<'_, [u8]>, Cow<'_, [u8]>>()
            .map_err(|error| mdbx_error("advance source scan cursor", error))?;
    }
    Ok(ScanBatch {
        rows,
        last_key: saw_row
            .then_some(batch_last_key)
            .or_else(|| last_key.map(ToOwned::to_owned)),
        finished,
        scanned,
    })
}

fn append_batch(
    destination: &Database<NoWriteMap>,
    table_name: Option<&str>,
    table_flags: TableFlags,
    rows: &[RetainedRow],
) -> StorageResult<()> {
    let transaction = destination
        .begin_rw_txn()
        .map_err(|error| mdbx_error("begin destination append transaction", error))?;
    let table = transaction
        .create_table(table_name, table_flags)
        .map_err(|error| mdbx_error("open destination append table", error))?;
    let mut cursor = transaction
        .cursor(&table)
        .map_err(|error| mdbx_error("open destination append cursor", error))?;
    for row in rows {
        cursor
            .put(&row.key, &row.value, WriteFlags::APPEND)
            .map_err(|error| mdbx_error("append ordered destination row", error))?;
    }
    drop(cursor);
    transaction
        .commit()
        .map_err(|error| mdbx_error("commit destination append transaction", error))?;
    Ok(())
}

fn verify_destination(
    destination: &Database<NoWriteMap>,
    names: &[String],
    flags: &[TableFlags],
    descriptor_keys: &BTreeSet<Vec<u8>>,
    exclusion: &MdbxExactKeyExclusion,
    copied: Vec<TableCopy>,
) -> StorageResult<Vec<MdbxRebaseTableReport>> {
    let table_names = std::iter::once(None)
        .chain(names.iter().map(|name| Some(name.as_str())))
        .collect::<Vec<_>>();
    table_names
        .into_iter()
        .zip(flags.iter().copied())
        .zip(copied)
        .map(|((table_name, expected_flags), source)| {
            let (actual_flags, destination_stats, destination_digest) =
                digest_table(destination, table_name, descriptor_keys, exclusion)?;
            let source_digest = encode_hex(&source.digest.finalize());
            let destination_digest = encode_hex(&destination_digest.finalize());
            if actual_flags != expected_flags
                || destination_stats.logical_rows != source.stats.logical_rows
                || destination_stats.key_bytes != source.stats.key_bytes
                || destination_stats.value_bytes != source.stats.value_bytes
                || destination_digest != source_digest
                || destination_stats.excluded_rows != 0
            {
                return Err(StorageError::backend(format!(
                    "MDBX rebase verification failed for table {:?}: source rows/digest={}/{source_digest}, destination rows/digest={}/{destination_digest}, source flags={:#x}, destination flags={:#x}, destination excluded rows={}",
                    table_label(table_name),
                    source.stats.logical_rows,
                    destination_stats.logical_rows,
                    expected_flags.bits(),
                    actual_flags.bits(),
                    destination_stats.excluded_rows,
                )));
            }
            Ok(MdbxRebaseTableReport {
                table: table_label(table_name).to_owned(),
                source_rows: source.stats.raw_rows,
                destination_rows: destination_stats.raw_rows,
                copied_rows: source.stats.logical_rows,
                excluded_rows: source.stats.excluded_rows,
                descriptor_rows: source.stats.descriptor_rows,
                copied_key_bytes: source.stats.key_bytes,
                copied_value_bytes: source.stats.value_bytes,
                excluded_key_bytes: source.stats.excluded_key_bytes,
                excluded_value_bytes: source.stats.excluded_value_bytes,
                table_flags: expected_flags.bits(),
                ordered_sha256: source_digest,
            })
        })
        .collect()
}

fn digest_table(
    database: &Database<NoWriteMap>,
    table_name: Option<&str>,
    descriptor_keys: &BTreeSet<Vec<u8>>,
    exclusion: &MdbxExactKeyExclusion,
) -> StorageResult<(TableFlags, LogicalStats, Sha256)> {
    let transaction = database
        .begin_ro_txn()
        .map_err(|error| mdbx_error("begin destination verification", error))?;
    let table = transaction
        .open_table(table_name)
        .map_err(|error| mdbx_error("open destination verification table", error))?;
    let flags = raw_table_flags(&transaction, &table, table_name)?;
    let mut cursor = transaction
        .cursor(&table)
        .map_err(|error| mdbx_error("open destination verification cursor", error))?;
    let mut digest = TableCopy::new(table_name);
    let mut entry = cursor
        .first::<Cow<'_, [u8]>, Cow<'_, [u8]>>()
        .map_err(|error| mdbx_error("read first destination verification row", error))?;
    while let Some((key, value)) = entry {
        let key = key.as_ref();
        let value = value.as_ref();
        digest.stats.raw_rows = digest.stats.raw_rows.saturating_add(1);
        if table_name.is_none() && descriptor_keys.contains(key) {
            digest.stats.descriptor_rows = digest.stats.descriptor_rows.saturating_add(1);
        } else if exclusion.matches(table_name, key) {
            digest.stats.excluded_rows = digest.stats.excluded_rows.saturating_add(1);
        } else {
            digest.stats.logical_rows = digest.stats.logical_rows.saturating_add(1);
            digest.stats.key_bytes = digest.stats.key_bytes.saturating_add(key.len() as u64);
            digest.stats.value_bytes = digest.stats.value_bytes.saturating_add(value.len() as u64);
            if is_environment_id(table_name, key) && value.len() != 16 {
                return Err(StorageError::invalid_data(format!(
                    "destination MDBX environment identity has invalid length {}",
                    value.len()
                )));
            }
            let digest_value = if is_environment_id(table_name, key) {
                ENVIRONMENT_ID_DIGEST_PLACEHOLDER
            } else {
                value
            };
            hash_entry(&mut digest.digest, key, digest_value);
        }
        entry = cursor
            .next::<Cow<'_, [u8]>, Cow<'_, [u8]>>()
            .map_err(|error| mdbx_error("advance destination verification cursor", error))?;
    }
    Ok((flags, digest.stats, digest.digest))
}
