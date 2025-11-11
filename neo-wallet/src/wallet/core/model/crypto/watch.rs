use neo_base::hash::Hash160;

use crate::account::Account;
use crate::{account::Contract, wallet::core::model::wallet::Wallet, WalletError};

impl Wallet {
    pub fn add_watch_only(
        &mut self,
        script_hash: Hash160,
        contract: Option<Contract>,
        make_default: bool,
    ) -> Result<Hash160, WalletError> {
        let mut account = Account::watch_only_from_script(script_hash, contract);
        account.set_default(make_default);
        let hash = account.script_hash();
        self.add_account(account)?;
        if make_default {
            self.set_default_internal(&hash)?;
        }
        Ok(hash)
    }
}
