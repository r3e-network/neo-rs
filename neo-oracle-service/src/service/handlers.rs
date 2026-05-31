use super::utils::{ledger_height, wallet_has_oracle_account};
use super::{OracleService, OracleStatus};
use neo_core::neo_system::NeoSystem;
use neo_core::persistence::DataCache;
use neo_core::smart_contract::native::{Role, RoleManagement};
use neo_core::wallets::Wallet;
use std::sync::Arc;

impl neo_core::i_event_handlers::CommittingHandler for OracleService {
    fn blockchain_committing_handler(
        &self,
        system: &dyn std::any::Any,
        _block: &neo_core::ledger::Block,
        snapshot: &DataCache,
        _application_executed_list: &[neo_core::ledger::blockchain_application_executed::ApplicationExecuted],
    ) {
        let Some(system) = system.downcast_ref::<NeoSystem>() else {
            return;
        };
        if system.settings().network != self.settings.network {
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
        let oracles = RoleManagement::new()
            .get_designated_by_role_at(snapshot, Role::Oracle, height)
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

impl neo_core::i_event_handlers::WalletChangedHandler for OracleService {
    fn wallet_provider_wallet_changed_handler(
        &self,
        _sender: &dyn std::any::Any,
        wallet: Option<Arc<dyn Wallet>>,
    ) {
        *self.wallet.write() = wallet.clone();
        if self.settings.auto_start {
            if let Some(wallet) = wallet {
                if let Some(service) = self.self_ref.read().upgrade() {
                    service.start(wallet);
                }
            } else {
                self.stop();
            }
        }
    }
}
