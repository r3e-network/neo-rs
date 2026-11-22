pub mod containment;
pub mod controller;

use neo_ai_agent::{
    EvidenceAuthority, PROBE_SCHEMA_VERSION, ProbeAction, ProbeContractError, ProbeJob,
    ProbeOutcome, ProbeOutcomeStatus, valid_hash,
};
use neo_native_contracts::native::composition::default_native_services;
use neo_primitives::{CallFlags, TriggerType, Verifiable};
use neo_protocol::ProtocolSettings;
use neo_protocol::payloads::Transaction;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::diagnostic::Diagnostic;
use neo_storage::persistence::data_cache::PersistenceDataCache;
use neo_storage::persistence::{
    DetachedSnapshot, DetachedSnapshotEntry, DetachedSnapshotError, DetachedSnapshotLimits,
};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::{ExecutionContext, Instruction, VmState};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use thiserror::Error;

pub const MAX_FRAME_BYTES: usize = 512 * 1024;
const MAX_DETACHED_ENTRIES: usize = 4_096;
const MAX_DETACHED_BYTES: usize = 384 * 1024;
/// Deterministic sandbox GAS cap (in datoshi) granted per admitted millisecond
/// of the probe budget.
///
/// Wall-clock timing cannot be serialized into the outcome without breaking
/// the byte-identical replay contract (the controller compares two worker runs
/// for exact equality), so the duration budget is enforced through this
/// deterministic GAS cap instead.
const SANDBOX_GAS_PER_DURATION_MS: i64 = 10_000;
/// Maximum length of the fault message excerpt embedded in observations.
const FAULT_MESSAGE_EXCERPT_CHARS: usize = 160;

fn snapshot_entries_hash(entries: &[SnapshotEntry]) -> String {
    let mut sorted = entries.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| left.key.cmp(&right.key));
    let mut hasher = Sha256::new();
    hasher.update(b"neo-probe-detached-snapshot-v1");
    for entry in sorted {
        update_length_prefixed(&mut hasher, &entry.key);
        update_length_prefixed(&mut hasher, &entry.value);
        hasher.update([u8::from(entry.constant)]);
    }
    hex_digest(hasher.finalize().as_slice())
}

fn hash_parts(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"neo-probe-preflight-v1");
    for part in parts {
        update_length_prefixed(&mut hasher, part);
    }
    hex_digest(hasher.finalize().as_slice())
}

fn update_length_prefixed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(bytes);
}

fn hex_digest(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

mod frame;
mod worker;

pub use frame::{
    ProbeBindings, ProbeRequestFrame, SnapshotEntry, SnapshotInput, WorkerError, decode_frame,
    encode_frame,
};
pub use worker::{execute_request, run_worker_once};

#[cfg(test)]
mod tests;
