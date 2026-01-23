//! Oracle service implementation (parity with Neo.Plugins.OracleService).

mod handlers;
mod lifecycle;
mod processing;
mod transactions;
mod utils;

#[cfg(test)]
mod tests;

use crate::cryptography::ECPoint;
use crate::neo_system::NeoSystem;
use crate::network::p2p::payloads::Transaction;
use crate::wallets::Wallet;
use parking_lot::{Mutex, RwLock};
use std::collections::{BTreeMap, HashMap};
#[cfg(feature = "oracle")]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, AtomicU8};
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tokio::task::JoinHandle;

#[cfg(feature = "oracle")]
pub(super) use super::https::OracleHttpsProtocol;
#[cfg(feature = "oracle")]
pub(super) use super::neofs::OracleNeoFsProtocol;
pub(super) use super::OracleServiceSettings;

const REFRESH_INTERVAL: Duration = Duration::from_secs(3 * 60);
const FINISHED_CACHE_TTL: Duration = Duration::from_secs(3 * 24 * 60 * 60);
const FILTER_MAX_NEST: usize = 64;
#[cfg(feature = "oracle")]
const SIGNATURE_SEND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleStatus {
    Unstarted,
    Running,
    Stopped,
}

impl OracleStatus {
    fn as_u8(self) -> u8 {
        match self {
            OracleStatus::Unstarted => 0,
            OracleStatus::Running => 1,
            OracleStatus::Stopped => 2,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => OracleStatus::Running,
            2 => OracleStatus::Stopped,
            _ => OracleStatus::Unstarted,
        }
    }
}

#[derive(Debug, Error)]
pub enum OracleServiceError {
    #[error("oracle service disabled")]
    Disabled,
    #[error("oracle request already finished")]
    RequestFinished,
    #[error("oracle request not found")]
    RequestNotFound,
    #[error("oracle not designated: {0}")]
    NotDesignated(String),
    #[error("invalid signature: {0}")]
    InvalidSignature(String),
    #[error("invalid oracle public key")]
    InvalidOraclePublicKey,
    #[error("oracle request transaction not found")]
    RequestTransactionNotFound,
    #[error("oracle response build failed: {0}")]
    BuildFailed(String),
    #[error("oracle processing error: {0}")]
    Processing(String),
}

struct OracleTask {
    tx: Option<Transaction>,
    backup_tx: Option<Transaction>,
    signs: BTreeMap<ECPoint, Vec<u8>>,
    backup_signs: BTreeMap<ECPoint, Vec<u8>>,
    timestamp: SystemTime,
}

/// Oracle service runtime.
pub struct OracleService {
    settings: OracleServiceSettings,
    system: Arc<NeoSystem>,
    status: AtomicU8,
    self_ref: RwLock<Weak<OracleService>>,
    wallet: RwLock<Option<Arc<dyn Wallet>>>,
    pending_queue: Mutex<HashMap<u64, OracleTask>>,
    finished_cache: Mutex<HashMap<u64, SystemTime>>,
    cancel: AtomicBool,
    request_task: Mutex<Option<JoinHandle<()>>>,
    timer_task: Mutex<Option<JoinHandle<()>>>,
    #[cfg(feature = "oracle")]
    counter: AtomicU64,
    #[cfg(feature = "oracle")]
    https: OracleHttpsProtocol,
    #[cfg(feature = "oracle")]
    neofs: OracleNeoFsProtocol,
}
