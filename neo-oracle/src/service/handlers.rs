//! Host-node event hooks.
//!
//! Mirrors the reference implementation's `service/handlers.rs`, which wires
//! `OracleService` into neo-core's `CommittingHandler` and `WalletChangedHandler`
//! event traits. Those traits live in the host node, so here the same logic is
//! exposed as plain methods that the integration layer calls from its own
//! handler implementations.

use super::{
    OracleService, OracleSigner, OracleSnapshot, OracleStatus, OracleWallet, ResponseTxBuilder,
    SignatureVerifier, SnapshotFactory, TxRelay,
};
use std::sync::Arc;

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
    /// Committing-handler hook (parity with C# `CommittingHandler`).
    ///
    /// The host node calls this once per block commit, passing its own network
    /// magic so the service can ignore commits from a foreign network.
    pub fn handle_committing(&self, system_network: u32) {
        if system_network != self.settings.network {
            return;
        }

        if self.settings.auto_start
            && self.status() == OracleStatus::Unstarted
            && let Some(wallet) = self.wallet.read().clone()
            && let Some(service) = self.self_ref.read().upgrade()
        {
            service.start(wallet);
        }

        if self.status() != OracleStatus::Running {
            return;
        }

        let snapshot = self.snapshots.snapshot();
        let height = snapshot.ledger_height();
        let oracles = snapshot.designated_oracles(height).unwrap_or_default();
        if oracles.is_empty() {
            self.stop();
            return;
        }

        // Resolve the membership verdict before taking the write lock in `stop`,
        // which would otherwise deadlock against the read guard.
        let has_oracle_account = self
            .wallet
            .read()
            .as_ref()
            .is_some_and(|wallet| wallet.has_oracle_account(&oracles));
        if !has_oracle_account {
            self.stop();
        }
    }

    /// Wallet-changed hook (parity with C# `WalletChangedHandler`).
    pub fn handle_wallet_changed(&self, wallet: Option<Arc<W>>) {
        *self.wallet.write() = wallet.clone();
        if !self.settings.auto_start {
            return;
        }
        match wallet {
            Some(wallet) => {
                if let Some(service) = self.self_ref.read().upgrade() {
                    service.start(wallet);
                }
            }
            None => self.stop(),
        }
    }
}
