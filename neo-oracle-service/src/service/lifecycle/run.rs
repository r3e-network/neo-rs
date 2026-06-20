use super::super::utils::{ledger_height, wallet_has_oracle_account};
use super::super::{OracleService, OracleStatus};
use neo_native_contracts::{Role, RoleManagement};
use neo_wallets::Wallet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tracing::{info, warn};

impl OracleService {
    /// Start the oracle service with the wallet that owns a designated oracle key.
    pub fn start(self: &Arc<Self>, wallet: Arc<dyn Wallet>) {
        if self.is_running() {
            return;
        }

        let snapshot = self.snapshot_cache();
        let height = ledger_height(&snapshot);
        let oracles = match RoleManagement::new().get_designated_by_role_at(
            &snapshot,
            Role::Oracle,
            height,
        ) {
            Ok(oracles) => oracles,
            Err(err) => {
                warn!(target: "neo::oracle", %err, "failed to load designated oracle list");
                return;
            }
        };

        if oracles.is_empty() {
            warn!(target: "neo::oracle", "oracle service unavailable (no designated oracles)");
            return;
        }

        if !wallet_has_oracle_account(wallet.as_ref(), &oracles) {
            warn!(target: "neo::oracle", "oracle service unavailable (wallet has no oracle key)");
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

        info!(target: "neo::oracle", "oracle service started");
    }

    /// Stop the oracle service and cancel its background tasks.
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
        info!(target: "neo::oracle", "oracle service stopped");
    }
}
