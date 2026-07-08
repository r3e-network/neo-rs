//! Pending oracle response-signature task state.

use std::collections::BTreeMap;
use std::time::SystemTime;

use neo_crypto::ECPoint;
use neo_payloads::Transaction;

pub(super) struct OracleTask {
    pub(super) tx: Option<Transaction>,
    pub(super) backup_tx: Option<Transaction>,
    pub(super) signs: BTreeMap<ECPoint, Vec<u8>>,
    pub(super) backup_signs: BTreeMap<ECPoint, Vec<u8>>,
    pub(super) timestamp: SystemTime,
}
