use super::native_provider::{
    NativeOracleServiceProviderFactory, OracleServiceNativeProvider,
    OracleServiceNativeProviderFactory,
};
use super::utils::{ledger_height, wallet_has_oracle_account};
use super::{OracleService, OracleStatus};
use neo_config::ProtocolSettings;
use neo_storage::persistence::DataCache;
use neo_wallets::Wallet;
use std::sync::Arc;

impl neo_payloads::CommittingHandler for OracleService {
    fn blockchain_committing_handler(
        &self,
        system: &dyn std::any::Any,
        _block: &neo_payloads::Block,
        snapshot: &DataCache,
        _application_executed_list: &[neo_payloads::ApplicationExecuted],
    ) {
        let Some(settings) = system.downcast_ref::<ProtocolSettings>() else {
            return;
        };
        if settings.network != self.settings.network {
            return;
        }

        if self.settings.auto_start && self.status() == OracleStatus::Unstarted {
            if let Some(wallet) = self.wallet.read().clone() {
                if let Some(service) = self.self_ref.read().upgrade() {
                    service.start(wallet);
                }
            }
        }

        if self.status() != OracleStatus::Running {
            return;
        }

        let height = ledger_height(snapshot);
        let native = NativeOracleServiceProviderFactory.provider();
        let oracles = native
            .designated_oracles(snapshot, height)
            .unwrap_or_default();
        if oracles.is_empty() {
            self.stop();
            return;
        }

        let wallet = self.wallet.read();
        if let Some(wallet) = wallet.as_ref() {
            if !wallet_has_oracle_account(wallet.as_ref(), &oracles) {
                self.stop();
            }
        } else {
            self.stop();
        }
    }
}

impl neo_payloads::WalletChangedHandler for OracleService {
    fn wallet_provider_wallet_changed_handler(
        &self,
        _sender: &dyn std::any::Any,
        wallet: Option<Arc<dyn std::any::Any + Send + Sync>>,
    ) {
        // The leaf `WalletChangedHandler` trait is type-erased; the
        // concrete `Arc<dyn Wallet>` we want to store lives in
        // `neo_wallets`. Real callers (e.g. `Node`) should hand us a
        // proper `Arc<dyn Wallet>` downcasted before invoking this
        // handler. The trait cannot express that without a circular
        // dep, so the impl just best-effort casts: it tries the
        // downcast via the `Any` vtable and otherwise drops the
        // event.
        let wallet_arc: Option<Arc<dyn Wallet>> = wallet
            .as_ref()
            .and_then(|w| w.downcast_ref::<Arc<dyn Wallet>>().cloned());
        *self.wallet.write() = wallet_arc.clone();
        if self.settings.auto_start {
            if let Some(wallet) = wallet_arc {
                if let Some(service) = self.self_ref.read().upgrade() {
                    service.start(wallet);
                }
            } else {
                self.stop();
            }
        }
    }
}
