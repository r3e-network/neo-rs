//! Typed maintenance tables and codecs for durable sync sidecars.

use neo_payloads::Header;
use neo_primitives::UInt256;
use neo_storage::persistence::{Table, TableDecode, TableEncode, TableNamespace};
use neo_storage::{StorageError, StorageResult};

#[cfg(test)]
use neo_storage::persistence::IntoTableBytes;

use super::{HeaderStageWindow, MAX_VERIFIED_HEADER_WINDOW, SyncStageCheckpoint, SyncStageKind};
#[cfg(test)]
use crate::{ServiceError, ServiceResult};

const CHECKPOINT_KEY_PREFIX: &[u8] = b"neo.sync.stage-checkpoint.v1.";
const CHECKPOINT_KEY_LEN: usize = CHECKPOINT_KEY_PREFIX.len() + 1;
const CHECKPOINT_VALUE_MAGIC: &[u8; 6] = b"NRSCP1";
const CHECKPOINT_VALUE_LEN: usize = CHECKPOINT_VALUE_MAGIC.len() + 1 + 4 + 8 + 8;
const VERIFIED_HEADER_KEY_PREFIX: &[u8] = b"neo.sync.verified-header.v1.header.";
const VERIFIED_HEADER_KEY_LEN: usize = VERIFIED_HEADER_KEY_PREFIX.len() + 4;
const VERIFIED_WINDOW_KEY: &[u8] = b"neo.sync.verified-header.v1.window";
const VERIFIED_TARGET_HASH_KEY: &[u8] = b"neo.sync.verified-header.v1.target-hash";
const VERIFIED_WINDOW_MAGIC: &[u8; 6] = b"NRSHW1";
const VERIFIED_WINDOW_LEN: usize = VERIFIED_WINDOW_MAGIC.len() + 4 + 4;

#[derive(Debug)]
pub(super) struct CheckpointKeyCodec;

impl TableEncode<SyncStageKind> for CheckpointKeyCodec {
    type Encoded<'a> = [u8; CHECKPOINT_KEY_LEN];

    fn encode(stage: &SyncStageKind) -> StorageResult<Self::Encoded<'_>> {
        let mut bytes = [0_u8; CHECKPOINT_KEY_LEN];
        bytes[..CHECKPOINT_KEY_PREFIX.len()].copy_from_slice(CHECKPOINT_KEY_PREFIX);
        bytes[CHECKPOINT_KEY_PREFIX.len()] = stage.code();
        Ok(bytes)
    }
}

#[derive(Debug)]
pub(super) struct CheckpointValueCodec;

impl TableEncode<SyncStageCheckpoint> for CheckpointValueCodec {
    type Encoded<'a> = [u8; CHECKPOINT_VALUE_LEN];

    fn encode(checkpoint: &SyncStageCheckpoint) -> StorageResult<Self::Encoded<'_>> {
        let mut bytes = [0_u8; CHECKPOINT_VALUE_LEN];
        let magic_end = CHECKPOINT_VALUE_MAGIC.len();
        bytes[..magic_end].copy_from_slice(CHECKPOINT_VALUE_MAGIC);
        bytes[magic_end] = checkpoint.stage.code();
        bytes[magic_end + 1..magic_end + 5].copy_from_slice(&checkpoint.height.to_be_bytes());
        bytes[magic_end + 5..magic_end + 13]
            .copy_from_slice(&checkpoint.processed_blocks.to_be_bytes());
        bytes[magic_end + 13..].copy_from_slice(&checkpoint.changed_bytes.to_be_bytes());
        Ok(bytes)
    }
}

impl TableDecode<SyncStageCheckpoint> for CheckpointValueCodec {
    fn decode(bytes: &[u8]) -> StorageResult<SyncStageCheckpoint> {
        if bytes.len() != CHECKPOINT_VALUE_LEN
            || &bytes[..CHECKPOINT_VALUE_MAGIC.len()] != CHECKPOINT_VALUE_MAGIC
        {
            return Err(StorageError::invalid_data(format!(
                "invalid sync checkpoint payload: {} bytes",
                bytes.len()
            )));
        }

        let mut cursor = CHECKPOINT_VALUE_MAGIC.len();
        let stage_code = bytes[cursor];
        cursor += 1;
        let stage = SyncStageKind::from_code(stage_code).ok_or_else(|| {
            StorageError::invalid_data(format!("invalid sync checkpoint stage code {stage_code}"))
        })?;
        let height = read_u32(bytes, &mut cursor, "sync checkpoint")?;
        let processed_blocks = read_u64(bytes, &mut cursor, "sync checkpoint")?;
        let changed_bytes = read_u64(bytes, &mut cursor, "sync checkpoint")?;
        Ok(SyncStageCheckpoint::new(stage, height).with_counters(processed_blocks, changed_bytes))
    }
}

/// Durable progress records keyed by sync stage.
#[derive(Debug)]
pub(super) struct SyncCheckpointTable;

impl Table for SyncCheckpointTable {
    type Key = SyncStageKind;
    type Value = SyncStageCheckpoint;
    type KeyCodec = CheckpointKeyCodec;
    type ValueCodec = CheckpointValueCodec;

    const NAME: &'static str = "SyncStageCheckpoints";
    const NAMESPACE: TableNamespace = TableNamespace::Maintenance;
}

#[derive(Debug)]
pub(super) struct VerifiedHeaderKeyCodec;

impl TableEncode<u32> for VerifiedHeaderKeyCodec {
    type Encoded<'a> = [u8; VERIFIED_HEADER_KEY_LEN];

    fn encode(height: &u32) -> StorageResult<Self::Encoded<'_>> {
        let mut bytes = [0_u8; VERIFIED_HEADER_KEY_LEN];
        bytes[..VERIFIED_HEADER_KEY_PREFIX.len()].copy_from_slice(VERIFIED_HEADER_KEY_PREFIX);
        bytes[VERIFIED_HEADER_KEY_PREFIX.len()..].copy_from_slice(&height.to_be_bytes());
        Ok(bytes)
    }
}

/// One validated header plus the canonical bytes already produced during
/// staging. Retaining those bytes prevents a second serialization at commit.
#[derive(Debug)]
pub(super) struct StoredVerifiedHeader {
    header: Header,
    bytes: Vec<u8>,
}

impl StoredVerifiedHeader {
    pub(super) const fn new(header: Header, bytes: Vec<u8>) -> Self {
        Self { header, bytes }
    }

    pub(super) fn into_header(self) -> Header {
        self.header
    }

    pub(super) const fn header(&self) -> &Header {
        &self.header
    }
}

#[derive(Debug)]
pub(super) struct VerifiedHeaderValueCodec;

impl TableEncode<StoredVerifiedHeader> for VerifiedHeaderValueCodec {
    type Encoded<'a> = &'a [u8];

    fn encode(value: &StoredVerifiedHeader) -> StorageResult<Self::Encoded<'_>> {
        Ok(value.bytes.as_slice())
    }
}

impl TableDecode<StoredVerifiedHeader> for VerifiedHeaderValueCodec {
    fn decode(bytes: &[u8]) -> StorageResult<StoredVerifiedHeader> {
        let header = Header::from_bytes(bytes).map_err(|error| {
            StorageError::invalid_data(format!("decode verified header: {error}"))
        })?;
        Ok(StoredVerifiedHeader::new(header, bytes.to_vec()))
    }
}

/// Height-indexed verified headers staged ahead of the canonical tip.
#[derive(Debug)]
pub(super) struct VerifiedHeaderTable;

impl Table for VerifiedHeaderTable {
    type Key = u32;
    type Value = StoredVerifiedHeader;
    type KeyCodec = VerifiedHeaderKeyCodec;
    type ValueCodec = VerifiedHeaderValueCodec;

    const NAME: &'static str = "VerifiedHeaders";
    const NAMESPACE: TableNamespace = TableNamespace::Maintenance;
}

#[derive(Debug)]
pub(super) struct VerifiedWindowKeyCodec;

impl TableEncode<()> for VerifiedWindowKeyCodec {
    type Encoded<'a> = &'static [u8];

    fn encode(_: &()) -> StorageResult<Self::Encoded<'_>> {
        Ok(VERIFIED_WINDOW_KEY)
    }
}

#[derive(Debug)]
pub(super) struct VerifiedWindowValueCodec;

impl TableEncode<HeaderStageWindow> for VerifiedWindowValueCodec {
    type Encoded<'a> = [u8; VERIFIED_WINDOW_LEN];

    fn encode(window: &HeaderStageWindow) -> StorageResult<Self::Encoded<'_>> {
        if window.target_hash.is_some() {
            return Err(StorageError::invalid_operation(
                "verified-header window row must not duplicate the target-hash row",
            ));
        }
        validate_window_bounds(window.base_height, window.target_height)?;
        let mut bytes = [0_u8; VERIFIED_WINDOW_LEN];
        let magic_end = VERIFIED_WINDOW_MAGIC.len();
        bytes[..magic_end].copy_from_slice(VERIFIED_WINDOW_MAGIC);
        bytes[magic_end..magic_end + 4].copy_from_slice(&window.base_height.to_be_bytes());
        bytes[magic_end + 4..].copy_from_slice(&window.target_height.to_be_bytes());
        Ok(bytes)
    }
}

impl TableDecode<HeaderStageWindow> for VerifiedWindowValueCodec {
    fn decode(bytes: &[u8]) -> StorageResult<HeaderStageWindow> {
        if bytes.len() != VERIFIED_WINDOW_LEN {
            return Err(StorageError::invalid_data(format!(
                "invalid verified-header window payload: {} bytes",
                bytes.len()
            )));
        }
        if &bytes[..VERIFIED_WINDOW_MAGIC.len()] != VERIFIED_WINDOW_MAGIC {
            return Err(StorageError::invalid_data(
                "invalid verified-header window magic",
            ));
        }
        let mut base = [0_u8; 4];
        base.copy_from_slice(&bytes[VERIFIED_WINDOW_MAGIC.len()..VERIFIED_WINDOW_MAGIC.len() + 4]);
        let mut target = [0_u8; 4];
        target.copy_from_slice(&bytes[VERIFIED_WINDOW_MAGIC.len() + 4..]);
        let base_height = u32::from_be_bytes(base);
        let target_height = u32::from_be_bytes(target);
        validate_window_bounds(base_height, target_height)?;
        Ok(HeaderStageWindow {
            base_height,
            target_height,
            target_hash: None,
        })
    }
}

/// Singleton record describing the active fixed header window.
#[derive(Debug)]
pub(super) struct VerifiedHeaderWindowTable;

impl Table for VerifiedHeaderWindowTable {
    type Key = ();
    type Value = HeaderStageWindow;
    type KeyCodec = VerifiedWindowKeyCodec;
    type ValueCodec = VerifiedWindowValueCodec;

    const NAME: &'static str = "VerifiedHeaderWindow";
    const NAMESPACE: TableNamespace = TableNamespace::Maintenance;
}

#[derive(Debug)]
pub(super) struct TargetHashKeyCodec;

impl TableEncode<()> for TargetHashKeyCodec {
    type Encoded<'a> = &'static [u8];

    fn encode(_: &()) -> StorageResult<Self::Encoded<'_>> {
        Ok(VERIFIED_TARGET_HASH_KEY)
    }
}

#[derive(Debug)]
pub(super) struct TargetHashValueCodec;

impl TableEncode<UInt256> for TargetHashValueCodec {
    type Encoded<'a> = [u8; 32];

    fn encode(hash: &UInt256) -> StorageResult<Self::Encoded<'_>> {
        Ok(hash.as_bytes())
    }
}

impl TableDecode<UInt256> for TargetHashValueCodec {
    fn decode(bytes: &[u8]) -> StorageResult<UInt256> {
        UInt256::from_bytes(bytes).map_err(|error| {
            StorageError::invalid_data(format!("decode verified-header target hash: {error}"))
        })
    }
}

/// Singleton target hash published when a verified window reaches its target.
#[derive(Debug)]
pub(super) struct VerifiedHeaderTargetHashTable;

impl Table for VerifiedHeaderTargetHashTable {
    type Key = ();
    type Value = UInt256;
    type KeyCodec = TargetHashKeyCodec;
    type ValueCodec = TargetHashValueCodec;

    const NAME: &'static str = "VerifiedHeaderTargetHash";
    const NAMESPACE: TableNamespace = TableNamespace::Maintenance;
}

#[cfg(test)]
pub(super) fn checkpoint_key(stage: SyncStageKind) -> Vec<u8> {
    CheckpointKeyCodec::encode(&stage)
        .expect("sync stage key encoding is infallible")
        .into_table_bytes()
}

#[cfg(test)]
pub(super) fn verified_header_key(height: u32) -> Vec<u8> {
    VerifiedHeaderKeyCodec::encode(&height)
        .expect("verified-header key encoding is infallible")
        .into_table_bytes()
}

#[cfg(test)]
pub(super) fn encode_checkpoint(checkpoint: &SyncStageCheckpoint) -> Vec<u8> {
    CheckpointValueCodec::encode(checkpoint)
        .expect("checkpoint encoding is infallible")
        .into_table_bytes()
}

#[cfg(test)]
pub(super) fn decode_checkpoint(
    expected_stage: SyncStageKind,
    bytes: &[u8],
) -> ServiceResult<SyncStageCheckpoint> {
    let checkpoint = CheckpointValueCodec::decode(bytes)
        .map_err(|error| ServiceError::invalid_state(error.to_string()))?;
    if checkpoint.stage != expected_stage {
        return Err(ServiceError::invalid_state(format!(
            "sync checkpoint stage mismatch: requested {}, stored {}",
            expected_stage.as_str(),
            checkpoint.stage.as_str()
        )));
    }
    Ok(checkpoint)
}

fn validate_window_bounds(base_height: u32, target_height: u32) -> StorageResult<()> {
    if target_height <= base_height {
        return Err(StorageError::invalid_data(format!(
            "verified-header target {target_height} must be above canonical height {base_height}"
        )));
    }
    if target_height.saturating_sub(base_height) > MAX_VERIFIED_HEADER_WINDOW {
        return Err(StorageError::invalid_data(format!(
            "verified-header window exceeds the {MAX_VERIFIED_HEADER_WINDOW}-header limit"
        )));
    }
    Ok(())
}

fn read_u32(bytes: &[u8], cursor: &mut usize, record: &'static str) -> StorageResult<u32> {
    let slice = take(bytes, cursor, 4, record)?;
    let mut out = [0_u8; 4];
    out.copy_from_slice(slice);
    Ok(u32::from_be_bytes(out))
}

fn read_u64(bytes: &[u8], cursor: &mut usize, record: &'static str) -> StorageResult<u64> {
    let slice = take(bytes, cursor, 8, record)?;
    let mut out = [0_u8; 8];
    out.copy_from_slice(slice);
    Ok(u64::from_be_bytes(out))
}

fn take<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
    record: &'static str,
) -> StorageResult<&'a [u8]> {
    let start = *cursor;
    let end = start
        .checked_add(len)
        .ok_or_else(|| StorageError::invalid_data(format!("{record} decode cursor overflow")))?;
    let slice = bytes.get(start..end).ok_or_else(|| {
        StorageError::invalid_data(format!(
            "truncated {record}: need {len} bytes at offset {start}, got {} bytes",
            bytes.len()
        ))
    })?;
    *cursor = end;
    Ok(slice)
}
