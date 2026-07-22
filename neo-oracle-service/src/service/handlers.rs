use super::providers::{OracleContractReadProvider, OracleServiceNativeProvider};
use super::utils::{ledger_height, wallet_has_oracle_account};
use super::{OracleRuntimeProvider, OracleService, OracleStatus};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::persistence::DataCache;
use neo_wallets::Nep6Wallet;
use std::sync::Arc;

impl<R, P> neo_runtime::CommittingHandler for OracleService<R, P>
where
    R: OracleRuntimeProvider + 'static,
    P: NativeContractProvider + OracleContractReadProvider + 'static,
{
    fn blockchain_committing_handler<B: neo_storage::CacheRead>(
        &self,
        network: u32,
        _block: &neo_payloads::Block,
        snapshot: &DataCache<B>,
        _application_executed_list: &[neo_payloads::ApplicationExecuted],
    ) {
        if network != self.settings.network {
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
        let native = self.native_provider();
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

impl<R, P> neo_runtime::WalletChangedHandler for OracleService<R, P>
where
    R: OracleRuntimeProvider + 'static,
    P: NativeContractProvider + OracleContractReadProvider + 'static,
{
    type Sender = ();
    type Wallet = Nep6Wallet;

    fn wallet_provider_wallet_changed_handler(
        &self,
        _sender: &Self::Sender,
        wallet: Option<Arc<Self::Wallet>>,
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
