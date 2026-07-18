//! Low-level filesystem, digest, and MDBX helpers for offline rebasing.

#![allow(unsafe_code)]

use super::super::store::{ENVIRONMENT_ID_KEY, MAINTENANCE_TABLE};
use super::implementation::MDBX_REBASE_INCOMPLETE_FILE;
use crate::{StorageError, StorageResult};
use libmdbx::{Database, Error as MdbxError, NoWriteMap, TableFlags};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
};

pub(super) fn create_incomplete_destination(destination: &Path) -> StorageResult<()> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(StorageError::invalid_operation(format!(
            "MDBX rebase destination parent {} does not exist",
            parent.display()
        )));
    }
    fs::create_dir(destination).map_err(|error| {
        StorageError::io(format!(
            "create MDBX rebase destination {}: {error}",
            destination.display()
        ))
    })?;
    let sentinel_path = destination.join(MDBX_REBASE_INCOMPLETE_FILE);
    let mut sentinel = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&sentinel_path)
        .map_err(|error| StorageError::io(format!("create rebase sentinel: {error}")))?;
    sentinel
        .write_all(b"incomplete\n")
        .and_then(|()| sentinel.sync_all())
        .map_err(|error| StorageError::io(format!("sync rebase sentinel: {error}")))?;
    sync_directory(destination)?;
    sync_directory(parent)
}

pub(super) fn remove_incomplete_sentinel(destination: &Path) -> StorageResult<()> {
    fs::remove_file(destination.join(MDBX_REBASE_INCOMPLETE_FILE))
        .map_err(|error| StorageError::io(format!("remove rebase sentinel: {error}")))?;
    sync_directory(destination)
}

pub(super) fn sync_directory(path: &Path) -> StorageResult<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| StorageError::io(format!("sync directory {}: {error}", path.display())))
}

pub(super) fn database_file_bytes(path: &Path) -> StorageResult<u64> {
    fs::metadata(path.join("mdbx.dat"))
        .map(|metadata| metadata.len())
        .map_err(|error| StorageError::io(format!("stat {}: {error}", path.display())))
}

pub(super) fn read_environment_id(database: &Database<NoWriteMap>) -> StorageResult<[u8; 16]> {
    let transaction = database
        .begin_ro_txn()
        .map_err(|error| mdbx_error("begin environment identity read", error))?;
    let table = transaction
        .open_table(Some(MAINTENANCE_TABLE))
        .map_err(|error| mdbx_error("open maintenance table for environment identity", error))?;
    let bytes = transaction
        .get::<Vec<u8>>(&table, ENVIRONMENT_ID_KEY)
        .map_err(|error| mdbx_error("read environment identity", error))?
        .ok_or_else(|| StorageError::invalid_data("MDBX environment identity is absent"))?;
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        StorageError::invalid_data(format!(
            "MDBX environment identity has invalid length {}",
            bytes.len()
        ))
    })
}

pub(super) fn is_environment_id(table_name: Option<&str>, key: &[u8]) -> bool {
    table_name == Some(MAINTENANCE_TABLE) && key == ENVIRONMENT_ID_KEY
}

pub(super) fn hash_entry(digest: &mut Sha256, key: &[u8], value: &[u8]) {
    hash_length_prefixed(digest, key);
    hash_length_prefixed(digest, value);
}

pub(super) fn hash_length_prefixed(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
}

pub(super) fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

pub(super) fn table_label(table_name: Option<&str>) -> &str {
    table_name.unwrap_or("main")
}

pub(super) fn mdbx_error(context: &str, error: MdbxError) -> StorageError {
    StorageError::backend(format!("{context}: {error}"))
}

pub(super) fn raw_table_flags(
    transaction: &libmdbx::Transaction<'_, libmdbx::RO, NoWriteMap>,
    table: &libmdbx::Table<'_>,
    table_name: Option<&str>,
) -> StorageResult<TableFlags> {
    let mut flags = 0u32;
    let mut state = 0u32;
    // SAFETY: both raw handles remain owned by the live read transaction for
    // this call. The wrapper passes a null state pointer, which libmdbx
    // 0.12.13 rejects; supplying both documented outputs is required.
    let result = unsafe {
        mdbx_sys::mdbx_dbi_flags_ex(transaction.txn().0, table.dbi(), &mut flags, &mut state)
    };
    if result != mdbx_sys::MDBX_SUCCESS {
        return Err(mdbx_error(
            &format!("read table flags for {:?}", table_label(table_name)),
            MdbxError::from_err_code(result),
        ));
    }
    Ok(TableFlags::from_bits_truncate(flags))
}
