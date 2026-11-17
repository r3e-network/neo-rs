//! Durable operation journal and consensus-slot reservation.
//!
//! Anti-double-sign has to survive a restart: an in-memory map is erased by a
//! crash or an operator restart, after which the signer would happily sign a
//! second, *different* consensus message for a slot it already signed. This
//! module keeps the journal on disk as an append-only log of JSON records in
//! the enclave's sealed-data directory:
//!
//! * every state transition (`in_flight` → `succeeded`/`failed`) is appended
//!   and `fsync`ed before the signer hands out the result, so a completed
//!   operation is recoverable after a restart and is never re-signed;
//! * every record that belongs to a consensus slot carries the slot, so the
//!   slot → request binding is rebuilt on startup and a conflicting request
//!   for the same `(network, height, view, phase)` is refused;
//! * a torn trailing record (crash during `write`) is truncated on startup,
//!   never past a complete record; a corrupted record anywhere else fails the
//!   boot closed.
//!
//! ## Consensus slot encoding
//!
//! The wire request ([`RemoteSignRequest`]) carries the network id but not the
//! consensus position. Consensus requests therefore carry the position in the
//! canonical payload: `SignPayload::Canonical` with an `encoding` of the form
//! `neo-consensus/<version>/<phase>` (for example
//! `neo-consensus/v1/commit`) whose bytes start with the big-endian block
//! height (4 bytes) followed by the view number (1 byte) and the message.
//! Payloads that do not carry a decodable position (for example a bare
//! [`SignPayload::Digest`]) stay protected by the durable request-id journal,
//! but cannot be slot-checked - the wire format does not expose the position.

use anyhow::{Context, Result, bail};
use neo_wallets::signing::{
    RemoteSignRejection, RemoteSignRequest, RemoteSignResponse, SignPayload, SignPurpose,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::warn;

/// File name of the append-only journal inside the sealed-data directory.
pub const JOURNAL_FILE_NAME: &str = "operation-journal.log";

/// Prefix of the canonical encoding that carries a consensus position.
pub const CONSENSUS_ENCODING_PREFIX: &str = "neo-consensus";

/// Bytes of consensus position carried by a slot-tagged canonical payload
/// (4-byte big-endian block height followed by the view number).
pub const CONSENSUS_POSITION_BYTES: usize = 5;

/// The consensus position one signature authorizes.
///
/// A proposer legitimately signs several messages for the same
/// `(height, view)` (prepare, commit, ...), so the message phase is part of the
/// slot: two different payloads for the *same* `(network, height, view, phase)`
/// are an equivocation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ConsensusSlot {
    /// Neo network identifier.
    pub network_id: u32,
    /// Block height the message belongs to.
    pub block_height: u32,
    /// Consensus view number within the height.
    pub view_number: u8,
    /// Consensus message phase, e.g. `commit`.
    pub phase: String,
}

impl ConsensusSlot {
    /// Builds a slot explicitly.
    #[must_use]
    pub fn new(network_id: u32, block_height: u32, view_number: u8, phase: &str) -> Self {
        Self {
            network_id,
            block_height,
            view_number,
            phase: phase.to_ascii_lowercase(),
        }
    }

    /// Derives the slot a request is bound to, when the request carries one.
    ///
    /// Only consensus-purpose requests with a
    /// [`CONSENSUS_ENCODING_PREFIX`]-tagged canonical payload are slot bound;
    /// everything else returns `None`.
    #[must_use]
    pub fn from_request(request: &RemoteSignRequest) -> Option<Self> {
        if request.purpose != SignPurpose::Consensus {
            return None;
        }
        let SignPayload::Canonical { encoding, bytes } = &request.payload else {
            return None;
        };
        let phase = consensus_phase(encoding)?;
        if bytes.len() < CONSENSUS_POSITION_BYTES {
            return None;
        }
        let block_height = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        Some(Self {
            network_id: request.network,
            block_height,
            view_number: bytes[4],
            phase,
        })
    }

    /// Human-readable form used in rejection messages and logs.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "network {} height {} view {} phase '{}'",
            self.network_id, self.block_height, self.view_number, self.phase
        )
    }
}

/// Extracts the phase from a `neo-consensus/<version>/<phase>` encoding.
fn consensus_phase(encoding: &str) -> Option<String> {
    let rest = encoding.strip_prefix(CONSENSUS_ENCODING_PREFIX)?;
    let rest = rest.strip_prefix('/')?;
    let phase = rest.rsplit('/').next()?.trim();
    if phase.is_empty() {
        None
    } else {
        Some(phase.to_ascii_lowercase())
    }
}

/// Durable state of one dispatched signing request.
pub enum JournalState {
    /// Recorded but not yet signed (deferred dispatch or a crash mid-signing).
    InFlight { request: RemoteSignRequest },
    /// Durably completed; replayed verbatim on re-dispatch and query.
    Succeeded(RemoteSignResponse),
    /// Durably and permanently rejected; replayed verbatim.
    Failed(RemoteSignRejection),
}

/// One journaled request.
pub struct JournalEntry {
    /// Digest binding the request content; re-use with another digest is an
    /// idempotency conflict.
    pub request_digest: [u8; 32],
    /// Durable state of the operation.
    pub state: JournalState,
}

/// One line of the append-only journal.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case")]
enum JournalRecord {
    InFlight {
        request_id: String,
        request_digest: [u8; 32],
        request: RemoteSignRequest,
        #[serde(default)]
        slot: Option<ConsensusSlot>,
    },
    Succeeded {
        request_id: String,
        request_digest: [u8; 32],
        response: RemoteSignResponse,
        #[serde(default)]
        slot: Option<ConsensusSlot>,
    },
    Failed {
        request_id: String,
        request_digest: [u8; 32],
        rejection: RemoteSignRejection,
        #[serde(default)]
        slot: Option<ConsensusSlot>,
    },
}

/// Append-only, restart-surviving operation journal.
pub struct Journal {
    path: PathBuf,
    file: File,
    entries: BTreeMap<String, JournalEntry>,
    slots: BTreeMap<ConsensusSlot, String>,
}

impl Journal {
    /// Opens (or creates) the journal inside `sealed_data_dir` and replays it.
    pub fn open(sealed_data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(sealed_data_dir).with_context(|| {
            format!(
                "failed to create the sealed-data directory {}",
                sealed_data_dir.display()
            )
        })?;
        let path = sealed_data_dir.join(JOURNAL_FILE_NAME);

        let mut entries = BTreeMap::new();
        let mut slots = BTreeMap::new();
        let mut tail = JournalTail::Clean;
        if path.exists() {
            let data = std::fs::read(&path)
                .with_context(|| format!("failed to read the journal {}", path.display()))?;
            let replayed = replay(&data, &path)?;
            entries = replayed.entries;
            slots = replayed.slots;
            tail = replayed.tail;
            if let JournalTail::Torn { valid_length } = tail {
                drop_torn_tail(&path, data.len() as u64, valid_length)?;
            }
        }

        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to open the journal {}", path.display()))?;
        if tail == JournalTail::Unterminated {
            // Without the terminator the next append would share a line with
            // this record and both would be unreadable on the next boot.
            file.write_all(b"\n")
                .and_then(|()| file.sync_data())
                .with_context(|| {
                    format!(
                        "failed to terminate the last journal record in {}",
                        path.display()
                    )
                })?;
        }
        Ok(Self {
            path,
            file,
            entries,
            slots,
        })
    }

    /// Location of the journal file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Number of journaled requests currently held in memory.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when nothing has been journaled yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Looks up one journaled request.
    #[must_use]
    pub fn get(&self, request_id: &str) -> Option<&JournalEntry> {
        self.entries.get(request_id)
    }

    /// Looks up one journaled request mutably.
    #[must_use]
    pub fn get_mut(&mut self, request_id: &str) -> Option<&mut JournalEntry> {
        self.entries.get_mut(request_id)
    }

    /// Request id that already occupies `slot`, if any.
    #[must_use]
    pub fn slot_owner(&self, slot: &ConsensusSlot) -> Option<&str> {
        self.slots.get(slot).map(String::as_str)
    }

    /// Durably records that the request is being processed. Must succeed
    /// before the sealed key is used.
    pub fn record_in_flight(
        &mut self,
        request: &RemoteSignRequest,
        slot: Option<&ConsensusSlot>,
    ) -> Result<()> {
        self.append(JournalRecord::InFlight {
            request_id: request.request_id.clone(),
            request_digest: request.request_digest,
            request: request.clone(),
            slot: slot.cloned(),
        })
    }

    /// Durably records a completed operation.
    pub fn record_succeeded(
        &mut self,
        request: &RemoteSignRequest,
        response: &RemoteSignResponse,
        slot: Option<&ConsensusSlot>,
    ) -> Result<()> {
        self.append(JournalRecord::Succeeded {
            request_id: request.request_id.clone(),
            request_digest: request.request_digest,
            response: response.clone(),
            slot: slot.cloned(),
        })
    }

    /// Durably records a permanent rejection.
    pub fn record_failed(
        &mut self,
        request: &RemoteSignRequest,
        rejection: &RemoteSignRejection,
        slot: Option<&ConsensusSlot>,
    ) -> Result<()> {
        self.append(JournalRecord::Failed {
            request_id: request.request_id.clone(),
            request_digest: request.request_digest,
            rejection: rejection.clone(),
            slot: slot.cloned(),
        })
    }

    /// Appends one record, forces it to stable storage, then applies it to the
    /// in-memory view. A failed append leaves memory untouched, so the caller
    /// answers ambiguously instead of diverging from the disk.
    fn append(&mut self, record: JournalRecord) -> Result<()> {
        let mut line =
            serde_json::to_vec(&record).context("failed to encode an operation journal record")?;
        line.push(b'\n');
        self.file
            .write_all(&line)
            .with_context(|| format!("failed to append to the journal {}", self.path.display()))?;
        self.file
            .flush()
            .with_context(|| format!("failed to flush the journal {}", self.path.display()))?;
        self.file.sync_data().with_context(|| {
            format!(
                "failed to fsync the journal {} (durability is required for anti-double-sign)",
                self.path.display()
            )
        })?;
        apply_record(&mut self.entries, &mut self.slots, record);
        Ok(())
    }
}

struct ReplayedJournal {
    entries: BTreeMap<String, JournalEntry>,
    slots: BTreeMap<ConsensusSlot, String>,
    tail: JournalTail,
}

/// State of the end of the journal file after a replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JournalTail {
    /// The file is empty or ends with a terminated record.
    Clean,
    /// The last record is complete but its line terminator is missing.
    Unterminated,
    /// An incomplete record follows the first `valid_length` bytes.
    Torn { valid_length: u64 },
}

/// Cuts a torn trailing record off the journal and forces the cut to disk.
///
/// `valid_length` is the start of the torn record, so every byte that is kept
/// belongs to a complete, terminated record.
fn drop_torn_tail(path: &Path, replayed_length: u64, valid_length: u64) -> Result<()> {
    let context = || {
        format!(
            "failed to drop the incomplete trailing journal record in {}",
            path.display()
        )
    };
    // The append handle cannot do this: on Windows `append` grants no right
    // to change the file length.
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .with_context(context)?;
    let current_length = file.metadata().with_context(context)?.len();
    if current_length != replayed_length {
        bail!(
            "the journal {} changed while it was being replayed ({replayed_length} bytes read, \
             {current_length} bytes on disk); refusing to truncate it",
            path.display()
        );
    }
    file.set_len(valid_length).with_context(context)?;
    file.sync_all().with_context(context)?;
    Ok(())
}

/// Replays the journal, tolerating (and truncating) only a torn trailing
/// record.
fn replay(data: &[u8], path: &Path) -> Result<ReplayedJournal> {
    let mut entries = BTreeMap::new();
    let mut slots = BTreeMap::new();
    let mut tail = JournalTail::Clean;
    let total = data.len();
    let mut offset = 0usize;
    while offset < total {
        let remaining = &data[offset..];
        let (line, next, terminated) = match remaining.iter().position(|byte| *byte == b'\n') {
            Some(index) => (&remaining[..index], offset + index + 1, true),
            None => (remaining, total, false),
        };
        let start = offset;
        offset = next;
        let Ok(text) = std::str::from_utf8(line) else {
            bail!(
                "the journal {} is corrupted at byte {start}: not valid UTF-8",
                path.display()
            );
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        match serde_json::from_str::<JournalRecord>(text) {
            Ok(record) => {
                apply_record(&mut entries, &mut slots, record);
                if !terminated {
                    warn!(
                        "the last record at byte {start} of {} has no line terminator",
                        path.display()
                    );
                    tail = JournalTail::Unterminated;
                }
            }
            Err(error) => {
                if !data[next..].iter().all(u8::is_ascii_whitespace) {
                    bail!(
                        "the journal {} is corrupted at byte {start}: {error}",
                        path.display()
                    );
                }
                warn!(
                    "dropping an incomplete trailing record at byte {start} of {}",
                    path.display()
                );
                return Ok(ReplayedJournal {
                    entries,
                    slots,
                    tail: JournalTail::Torn {
                        valid_length: start as u64,
                    },
                });
            }
        }
    }
    Ok(ReplayedJournal {
        entries,
        slots,
        tail,
    })
}

/// Applies one record to the in-memory view. The first request to claim a
/// slot keeps it; completion records never release it, because a slot must
/// stay reserved for the whole consensus round (and beyond, across restarts).
fn apply_record(
    entries: &mut BTreeMap<String, JournalEntry>,
    slots: &mut BTreeMap<ConsensusSlot, String>,
    record: JournalRecord,
) {
    match record {
        JournalRecord::InFlight {
            request_id,
            request_digest,
            request,
            slot,
        } => {
            reserve(slots, slot, &request_id);
            // A completed entry is never downgraded by a late in-flight record.
            let completed = matches!(
                entries.get(&request_id).map(|entry| &entry.state),
                Some(JournalState::Succeeded(_) | JournalState::Failed(_))
            );
            if !completed {
                entries.insert(
                    request_id,
                    JournalEntry {
                        request_digest,
                        state: JournalState::InFlight { request },
                    },
                );
            }
        }
        JournalRecord::Succeeded {
            request_id,
            request_digest,
            response,
            slot,
        } => {
            reserve(slots, slot, &request_id);
            entries.insert(
                request_id,
                JournalEntry {
                    request_digest,
                    state: JournalState::Succeeded(response),
                },
            );
        }
        JournalRecord::Failed {
            request_id,
            request_digest,
            rejection,
            slot,
        } => {
            reserve(slots, slot, &request_id);
            entries.insert(
                request_id,
                JournalEntry {
                    request_digest,
                    state: JournalState::Failed(rejection),
                },
            );
        }
    }
}

fn reserve(
    slots: &mut BTreeMap<ConsensusSlot, String>,
    slot: Option<ConsensusSlot>,
    request_id: &str,
) {
    if let Some(slot) = slot {
        slots.entry(slot).or_insert_with(|| request_id.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_wallets::signing::{DigestAlgorithm, KeyRef, REMOTE_SIGNER_PROTOCOL_VERSION};

    #[test]
    fn consensus_slots_are_derived_from_the_canonical_position() {
        let request = slot_request(
            "req-slot",
            "neo-consensus/v1/commit",
            &[0, 0, 0, 7, 3, 0xAA],
        );
        let slot = ConsensusSlot::from_request(&request).expect("slot");
        assert_eq!(slot.network_id, 860_833_102);
        assert_eq!(slot.block_height, 7);
        assert_eq!(slot.view_number, 3);
        assert_eq!(slot.phase, "commit");
        assert_eq!(
            slot.describe(),
            "network 860833102 height 7 view 3 phase 'commit'"
        );
    }

    #[test]
    fn digest_payloads_are_not_slot_bound() {
        let mut request = slot_request("req-digest", "neo-consensus/v1/commit", &[0, 0, 0, 7, 3]);
        request.payload = SignPayload::Digest {
            algorithm: DigestAlgorithm::Sha256,
            digest: [3; 32],
        };
        assert!(ConsensusSlot::from_request(&request).is_none());
    }

    #[test]
    fn short_or_foreign_payloads_are_not_slot_bound() {
        assert!(
            ConsensusSlot::from_request(&slot_request(
                "req-short",
                "neo-consensus/v1/commit",
                &[1, 2]
            ))
            .is_none()
        );
        assert!(
            ConsensusSlot::from_request(&slot_request(
                "req-foreign",
                "neo-batch/v1",
                &[0, 0, 0, 7, 3]
            ))
            .is_none()
        );
    }

    #[test]
    fn journal_replays_after_reopen_and_keeps_the_slot_reserved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let slot = ConsensusSlot::new(860_833_102, 12, 1, "commit");
        let request = slot_request("req-durable", "neo-consensus/v1/commit", &[0, 0, 0, 12, 1]);
        {
            let mut journal = Journal::open(dir.path()).expect("journal");
            journal
                .record_in_flight(&request, Some(&slot))
                .expect("in-flight record");
            journal
                .record_failed(&request, &rejection(&request), None)
                .expect("failed record");
            assert_eq!(journal.slot_owner(&slot), Some("req-durable"));
        }
        let journal = Journal::open(dir.path()).expect("reopened journal");
        assert_eq!(journal.len(), 1);
        assert_eq!(journal.slot_owner(&slot), Some("req-durable"));
        assert!(matches!(
            journal.get("req-durable").map(|entry| &entry.state),
            Some(JournalState::Failed(_))
        ));
    }

    #[test]
    fn a_torn_trailing_record_is_truncated_on_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let request = slot_request("req-torn", "neo-consensus/v1/commit", &[0, 0, 0, 3, 0]);
        {
            let mut journal = Journal::open(dir.path()).expect("journal");
            journal.record_in_flight(&request, None).expect("record");
        }
        let path = dir.path().join(JOURNAL_FILE_NAME);
        let mut text = std::fs::read_to_string(&path).expect("read journal");
        text.push_str("{\"record\":\"succeeded\",\"request_id\":\"trunc");
        std::fs::write(&path, text).expect("write torn journal");

        let mut journal = Journal::open(dir.path()).expect("reopened journal");
        assert_eq!(journal.len(), 1);
        // The truncated file is appendable again.
        journal.record_in_flight(&request, None).expect("append");
        assert_eq!(
            std::fs::read_to_string(&path)
                .expect("read journal")
                .lines()
                .count(),
            2
        );
    }

    #[test]
    fn truncation_stops_at_the_last_complete_record() {
        let dir = tempfile::tempdir().expect("tempdir");
        let slot = ConsensusSlot::new(860_833_102, 4, 0, "commit");
        let first = slot_request("req-first", "neo-consensus/v1/commit", &[0, 0, 0, 4, 0]);
        let second = slot_request("req-second", "neo-consensus/v1/commit", &[0, 0, 0, 5, 0]);
        {
            let mut journal = Journal::open(dir.path()).expect("journal");
            journal
                .record_in_flight(&first, Some(&slot))
                .expect("first record");
            journal
                .record_in_flight(&second, None)
                .expect("second record");
        }
        let path = dir.path().join(JOURNAL_FILE_NAME);
        let intact = std::fs::read(&path).expect("read journal");
        let mut torn = intact.clone();
        torn.extend_from_slice(b"{\"record\":\"in_flight\",\"request_id\":\"req-th");
        std::fs::write(&path, torn).expect("write torn journal");

        let journal = Journal::open(dir.path()).expect("reopened journal");
        assert_eq!(journal.len(), 2);
        assert_eq!(journal.slot_owner(&slot), Some("req-first"));
        // Only the torn bytes are gone; both complete records are untouched.
        assert_eq!(std::fs::read(&path).expect("read journal"), intact);
    }

    #[test]
    fn a_complete_record_without_its_terminator_survives_later_appends() {
        let dir = tempfile::tempdir().expect("tempdir");
        let slot = ConsensusSlot::new(860_833_102, 9, 2, "commit");
        let first = slot_request("req-first", "neo-consensus/v1/commit", &[0, 0, 0, 9, 2]);
        let second = slot_request("req-second", "neo-consensus/v1/commit", &[0, 0, 0, 10, 0]);
        {
            let mut journal = Journal::open(dir.path()).expect("journal");
            journal
                .record_in_flight(&first, Some(&slot))
                .expect("first record");
        }
        let path = dir.path().join(JOURNAL_FILE_NAME);
        let intact = std::fs::read(&path).expect("read journal");
        assert_eq!(intact.last(), Some(&b'\n'));
        std::fs::write(&path, &intact[..intact.len() - 1]).expect("drop the terminator");

        {
            let mut journal = Journal::open(dir.path()).expect("reopened journal");
            assert_eq!(journal.len(), 1);
            assert_eq!(journal.slot_owner(&slot), Some("req-first"));
            assert_eq!(std::fs::read(&path).expect("read journal"), intact);
            journal.record_in_flight(&second, None).expect("append");
        }

        // The append did not fuse with the unterminated record, so neither is
        // mistaken for a torn tail on the next boot.
        let journal = Journal::open(dir.path()).expect("journal after the append");
        assert_eq!(journal.len(), 2);
        assert_eq!(journal.slot_owner(&slot), Some("req-first"));
        assert!(journal.get("req-second").is_some());
    }

    #[test]
    fn a_corrupted_record_before_the_tail_fails_closed_and_keeps_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let request = slot_request("req-kept", "neo-consensus/v1/commit", &[0, 0, 0, 6, 0]);
        {
            let mut journal = Journal::open(dir.path()).expect("journal");
            journal.record_in_flight(&request, None).expect("record");
        }
        let path = dir.path().join(JOURNAL_FILE_NAME);
        let record = std::fs::read(&path).expect("read journal");
        let mut corrupted = b"{\"record\":\"succeeded\",\"request_id\":\"trunc\n".to_vec();
        corrupted.extend_from_slice(&record);
        std::fs::write(&path, &corrupted).expect("write corrupted journal");

        let error = Journal::open(dir.path())
            .err()
            .expect("a corrupted journal must not open");
        assert!(error.to_string().contains("corrupted at byte 0"), "{error}");
        assert_eq!(std::fs::read(&path).expect("read journal"), corrupted);
    }

    fn slot_request(request_id: &str, encoding: &str, bytes: &[u8]) -> RemoteSignRequest {
        RemoteSignRequest {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            request_id: request_id.to_string(),
            key_ref: KeyRef::new("remote", "validator-1", 2),
            network: 860_833_102,
            purpose: SignPurpose::Consensus,
            payload: SignPayload::Canonical {
                encoding: encoding.to_string(),
                bytes: bytes.to_vec(),
            },
            request_digest: [5; 32],
            policy_version: "policy-v1".to_string(),
            attestation_session_id: None,
        }
    }

    fn rejection(request: &RemoteSignRequest) -> RemoteSignRejection {
        RemoteSignRejection {
            schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
            request_id: request.request_id.clone(),
            key_ref: request.key_ref.clone(),
            request_digest: request.request_digest,
            policy_version: request.policy_version.clone(),
            service_identity: "signer.prod".to_string(),
            code: "policy_permanent_denial".to_string(),
            message: "test".to_string(),
            permanent: true,
            attestation_session_id: None,
            session_signature: None,
        }
    }
}
