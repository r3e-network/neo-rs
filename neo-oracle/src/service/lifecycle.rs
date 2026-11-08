//! Construction, lifecycle transitions and the unhandled-exception policy.
//!
//! Mirrors the reference implementation's `service/lifecycle/{state,run,error}.rs`.

use super::{
    ExpiringSet, FINISHED_CACHE_TTL, OracleDedupState, OracleService, OracleServiceError,
    OracleServiceSettings, OracleSigner, OracleSnapshot, OracleStatus, OracleWallet,
    ResponseTxBuilder, SignatureVerifier, SnapshotFactory, TxRelay,
};
use crate::https::OracleHttpsProtocol;
use crate::neofs::OracleNeoFsProtocol;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Weak};

impl<S, W, Sig, TB, R, V> OracleService<S, W, Sig, TB, R, V>
where
    S: SnapshotFactory + Send + Sync + 'static,
    S::Snapshot: OracleSnapshot,
    W: OracleWallet + Send + Sync + 'static,
    Sig: OracleSigner + Send + Sync + 'static,
    TB: ResponseTxBuilder + Send + Sync + 'static,
    R: TxRelay + Send + Sync + 'static,
    V: SignatureVerifier + Send + Sync + 'static,
{
    /// Creates the oracle service, normalizing the supplied settings.
    pub fn new(
        settings: OracleServiceSettings,
        snapshots: Arc<S>,
        tx_builder: Arc<TB>,
        relay: Arc<R>,
        verifier: Arc<V>,
    ) -> Result<Self, OracleServiceError> {
        let mut settings = settings;
        settings.normalize();
        Ok(Self {
            settings,
            status: AtomicU8::new(OracleStatus::Unstarted.as_u8()),
            self_ref: RwLock::new(Weak::new()),
            wallet: RwLock::new(None),
            signer: RwLock::new(None),
            snapshots,
            tx_builder,
            relay,
            verifier,
            pending_queue: Mutex::new(HashMap::new()),
            finished_cache: Mutex::new(ExpiringSet::new(FINISHED_CACHE_TTL)),
            dedup: Mutex::new(OracleDedupState::default()),
            cancel: AtomicBool::new(false),
            request_task: Mutex::new(None),
            timer_task: Mutex::new(None),
            counter: AtomicU64::new(1),
            https: OracleHttpsProtocol::new(),
            neofs: OracleNeoFsProtocol::new(),
        })
    }

    /// Wires the service's weak self-reference (call once after `Arc`-ing).
    pub fn set_self_ref(self: &Arc<Self>) {
        *self.self_ref.write() = Arc::downgrade(self);
    }

    /// Sets the wallet used for oracle account selection.
    pub fn set_wallet(&self, wallet: Option<Arc<W>>) {
        *self.wallet.write() = wallet;
    }

    /// Installs the signer used for oracle response signatures.
    ///
    /// Mirrors `set_signer_client`: the signer is startup-authenticated and
    /// immutable for the lifetime of the installation.
    pub fn set_signer(&self, signer: Option<Arc<Sig>>) {
        *self.signer.write() = signer;
    }

    /// Returns the normalized settings in effect.
    pub fn settings(&self) -> &OracleServiceSettings {
        &self.settings
    }

    /// Returns the NeoFS protocol client used for `neofs:` oracle requests.
    pub fn neofs(&self) -> &OracleNeoFsProtocol {
        &self.neofs
    }

    /// Returns the HTTPS protocol client used for `https:` oracle requests.
    pub fn https(&self) -> &OracleHttpsProtocol {
        &self.https
    }

    /// Returns the current lifecycle status.
    pub fn status(&self) -> OracleStatus {
        OracleStatus::from_u8(self.status.load(Ordering::SeqCst))
    }

    /// Returns true while the service is running.
    pub fn is_running(&self) -> bool {
        self.status() == OracleStatus::Running
    }

    /// Starts the service with the given oracle wallet: resolves the oracle
    /// designated set and spawns the request and timer tasks.
    pub fn start(self: &Arc<Self>, wallet: Arc<W>) {
        if self.is_running() {
            return;
        }

        let snapshot = self.snapshots.snapshot();
        let height = snapshot.ledger_height();
        let oracles = match snapshot.designated_oracles(height) {
            Ok(oracles) => oracles,
            Err(err) => {
                tracing::warn!(target: "neo::oracle", %err, "failed to load designated oracle list");
                return;
            }
        };

        if oracles.is_empty() {
            tracing::warn!(target: "neo::oracle", "oracle service unavailable (no designated oracles)");
            return;
        }

        if !wallet.has_oracle_account(&oracles) {
            tracing::warn!(target: "neo::oracle", "oracle service unavailable (wallet has no oracle key)");
            return;
        }

        *self.wallet.write() = Some(wallet);
        self.cancel.store(false, Ordering::SeqCst);
        self.status
            .store(OracleStatus::Running.as_u8(), Ordering::SeqCst);

        let request_task = {
            let service = Arc::clone(self);
            tokio::spawn(async move {
                service.process_requests_loop().await;
            })
        };

        let timer_task = {
            let service = Arc::clone(self);
            tokio::spawn(async move {
                service.timer_loop().await;
            })
        };

        *self.request_task.lock() = Some(request_task);
        *self.timer_task.lock() = Some(timer_task);

        tracing::info!(target: "neo::oracle", "oracle service started");
    }

    /// Stops the service, cancelling pending requests and timers.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        *self.wallet.write() = None;
        self.status
            .store(OracleStatus::Stopped.as_u8(), Ordering::SeqCst);
        self.pending_queue.lock().clear();
        if let Some(handle) = self.request_task.lock().take() {
            handle.abort();
        }
        if let Some(handle) = self.timer_task.lock().take() {
            handle.abort();
        }
        tracing::info!(target: "neo::oracle", "oracle service stopped");
    }

    /// Applies the configured unhandled-exception policy to `err`.
    pub fn handle_error(&self, err: &OracleServiceError) {
        tracing::error!(target: "neo::oracle", error = %err, "oracle service error");
        self.settings.exception_policy.apply(|| self.stop());
    }
}
