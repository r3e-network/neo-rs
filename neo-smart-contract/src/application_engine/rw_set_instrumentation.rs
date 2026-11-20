//! Opt-in read/write set instrumentation for speculative parallel execution.
//!
//! The engine records what a transaction observed and modified so a scheduler
//! can validate an optimistic block without re-executing every transaction.
//! Recording is off by default: [`ApplicationEngine::enable_rw_set_tracking`]
//! allocates the tracker, and until it is called every recording site is a
//! single relaxed atomic load that neither locks nor allocates.
//!
//! The persistence layer owns the lifecycle:
//!
//! ```text
//! engine.enable_rw_set_tracking();          // before executing a transaction
//! engine.set_rw_set_generation(committed);  // position it on the shared clock
//! engine.execute()?;
//! let set = engine.take_rw_set();           // hand the set to the validator
//! ```
//!
//! # Recorded observations
//!
//! * `System.Storage.Get` and every native/legacy storage read, including reads
//!   that returned nothing (an absence is a dependency too);
//! * `System.Storage.Put` and `System.Storage.Delete`, which are both writes;
//! * `System.Storage.Find` as a scanned prefix plus the entries it returned;
//! * notification and log emissions, which are not conflict keys but act as
//!   ordering barriers because they are observable outside the write set.

use super::*;
use neo_vm::RwKey;

/// Domain tag that keeps notification keys out of the log key space.
const EVENT_TAG_NOTIFY: u8 = 0x01;
/// Domain tag that keeps log keys out of the notification key space.
const EVENT_TAG_LOG: u8 = 0x02;

/// Builds the read-write set key of one contract storage entry.
///
/// The encoding matches [`StorageKey::to_array`] - little-endian contract id
/// followed by the key suffix - so a key built from `(id, suffix)` and one built
/// from a full [`StorageKey`] compare equal.
pub(crate) fn storage_rw_key(contract_id: i32, suffix: &[u8]) -> RwKey {
    let mut bytes = Vec::with_capacity(std::mem::size_of::<i32>() + suffix.len());
    bytes.extend_from_slice(&contract_id.to_le_bytes());
    bytes.extend_from_slice(suffix);
    RwKey::new(bytes)
}

/// Builds the key of a notification or log event.
///
/// Event payloads are unbounded, so the key is a fixed-size digest of the
/// emitting contract and the payload. Events never take part in conflict
/// detection; the key only identifies the emission for ordering purposes.
fn event_rw_key(tag: u8, script_hash: &UInt160, payload: &[u8]) -> RwKey {
    let mut buffer = Vec::with_capacity(1 + HASH_SIZE + payload.len());
    buffer.push(tag);
    buffer.extend_from_slice(&script_hash.as_bytes());
    buffer.extend_from_slice(payload);
    RwKey::new(Crypto::hash256(&buffer))
}

impl ApplicationEngine {
    /// Enables read-write set recording for this engine.
    ///
    /// Until this is called the engine allocates no tracker and every recording
    /// site is a single relaxed atomic load, so callers that never schedule
    /// speculatively pay neither allocations nor bookkeeping.
    pub fn enable_rw_set_tracking(&mut self) {
        *self.rw_set_tracker.get_mut() = Some(RwSetTracker::new());
        self.rw_set_tracking.store(true, Ordering::Relaxed);
    }

    /// Enables recording positioned at `generation`, the version of the state
    /// the transaction is about to observe.
    pub fn enable_rw_set_tracking_at_generation(&mut self, generation: u64) {
        *self.rw_set_tracker.get_mut() = Some(RwSetTracker::with_generation(generation));
        self.rw_set_tracking.store(true, Ordering::Relaxed);
    }

    /// Disables recording and drops everything recorded so far.
    pub fn disable_rw_set_tracking(&mut self) {
        *self.rw_set_tracker.get_mut() = None;
        self.rw_set_tracking.store(false, Ordering::Relaxed);
    }

    /// Returns whether read-write set recording is enabled.
    pub fn is_rw_set_tracking_enabled(&self) -> bool {
        self.rw_set_tracker.lock().is_some()
    }

    /// Positions the enabled tracker on the shared version clock.
    ///
    /// This is a no-op when recording is disabled. The persistence layer calls
    /// it with the committed set's `write_clock()` before executing, which makes
    /// the transaction's reads comparable with the committed writes.
    pub fn set_rw_set_generation(&self, generation: u64) {
        if let Some(tracker) = self.rw_set_tracker.lock().as_mut() {
            tracker.set_generation(generation);
        }
    }

    /// Returns a copy of the recorded set stamped with the gas consumed.
    pub fn rw_set(&self) -> Option<RwSetTracker> {
        let mut set = self.rw_set_tracker.lock().clone()?;
        set.set_gas_used(self.gas_consumed);
        Some(set)
    }

    /// Removes the recorded set, disabling further recording.
    pub fn take_rw_set(&mut self) -> Option<RwSetTracker> {
        self.rw_set_tracking.store(false, Ordering::Relaxed);
        let mut set = self.rw_set_tracker.get_mut().take()?;
        set.set_gas_used(self.gas_consumed);
        Some(set)
    }

    /// Records one storage read, `present == false` when the key held no value.
    pub(super) fn record_rw_storage_read(&self, contract_id: i32, suffix: &[u8], present: bool) {
        if !self.rw_set_tracking.load(Ordering::Relaxed) {
            return;
        }
        let mut guard = self.rw_set_tracker.lock();
        let Some(tracker) = guard.as_mut() else {
            return;
        };
        let key = storage_rw_key(contract_id, suffix);
        if present {
            tracker.record_read(key);
        } else {
            tracker.record_absent_read(key);
        }
    }

    /// Records one storage read through a full [`StorageKey`].
    pub(super) fn record_rw_storage_read_key(&self, key: &StorageKey) {
        if !self.rw_set_tracking.load(Ordering::Relaxed) {
            return;
        }
        let mut guard = self.rw_set_tracker.lock();
        let Some(tracker) = guard.as_mut() else {
            return;
        };
        tracker.record_read(storage_rw_key(key.id(), key.suffix()));
    }

    /// Records one storage write (`Put` or `Delete`).
    pub(super) fn record_rw_storage_write(&self, contract_id: i32, suffix: &[u8]) {
        if !self.rw_set_tracking.load(Ordering::Relaxed) {
            return;
        }
        let mut guard = self.rw_set_tracker.lock();
        let Some(tracker) = guard.as_mut() else {
            return;
        };
        tracker.record_write(storage_rw_key(contract_id, suffix));
    }

    /// Records one storage write through a full [`StorageKey`].
    pub(super) fn record_rw_storage_write_key(&self, key: &StorageKey) {
        if !self.rw_set_tracking.load(Ordering::Relaxed) {
            return;
        }
        let mut guard = self.rw_set_tracker.lock();
        let Some(tracker) = guard.as_mut() else {
            return;
        };
        tracker.record_write(storage_rw_key(key.id(), key.suffix()));
    }

    /// Records a scanned key range, so keys created inside it later conflict.
    pub(super) fn record_rw_prefix_read(&self, contract_id: i32, prefix: &[u8]) {
        if !self.rw_set_tracking.load(Ordering::Relaxed) {
            return;
        }
        let mut guard = self.rw_set_tracker.lock();
        let Some(tracker) = guard.as_mut() else {
            return;
        };
        tracker.record_read_prefix(storage_rw_key(contract_id, prefix));
    }

    /// Records an emitted notification (ordering barrier, not a conflict key).
    pub(super) fn record_rw_notification(&self, script_hash: &UInt160, event_name: &str) {
        if !self.rw_set_tracking.load(Ordering::Relaxed) {
            return;
        }
        let mut guard = self.rw_set_tracker.lock();
        let Some(tracker) = guard.as_mut() else {
            return;
        };
        tracker.record_notify_event(event_rw_key(
            EVENT_TAG_NOTIFY,
            script_hash,
            event_name.as_bytes(),
        ));
    }

    /// Records an emitted log message (ordering barrier, not a conflict key).
    pub(super) fn record_rw_log(&self, script_hash: &UInt160, message: &str) {
        if !self.rw_set_tracking.load(Ordering::Relaxed) {
            return;
        }
        let mut guard = self.rw_set_tracker.lock();
        let Some(tracker) = guard.as_mut() else {
            return;
        };
        tracker.record_log_event(event_rw_key(EVENT_TAG_LOG, script_hash, message.as_bytes()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_key_encoding_matches_storage_key_bytes() {
        let encoded = storage_rw_key(-5, b"entry");
        assert_eq!(
            encoded.as_bytes(),
            StorageKey::new(-5, b"entry".to_vec()).to_array()
        );
    }

    #[test]
    fn storage_key_encoding_keeps_contract_ids_apart() {
        assert_ne!(storage_rw_key(1, b"k"), storage_rw_key(2, b"k"));
        assert_ne!(storage_rw_key(-1, b"k"), storage_rw_key(1, b"k"));
    }

    #[test]
    fn event_keys_are_domain_separated_and_deterministic() {
        let hash = UInt160::from_bytes(&[0x11; 20]).expect("script hash");
        let notify = event_rw_key(EVENT_TAG_NOTIFY, &hash, b"Transfer");
        let log = event_rw_key(EVENT_TAG_LOG, &hash, b"Transfer");

        assert_ne!(notify, log);
        assert_eq!(notify, event_rw_key(EVENT_TAG_NOTIFY, &hash, b"Transfer"));
        assert_eq!(notify.len(), HASH_SIZE);
    }
}
