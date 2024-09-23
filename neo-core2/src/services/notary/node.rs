use std::sync::{Arc, Mutex, RwLock};
use std::collections::HashMap;
use slices::Contains;
use crate::crypto::keys::PublicKeys;
use crate::encoding::address;
use crate::util::Uint256;
use crate::wallet::{Account, Wallet};
use log::warn;

pub struct Notary {
    acc_mtx: Mutex<()>,
    req_mtx: Mutex<()>,
    curr_account: Option<Arc<Account>>,
    wallet: Arc<Wallet>,
    requests: RwLock<HashMap<Uint256, Request>>,
    config: Config,
}

impl Notary {
    pub fn update_notary_nodes(&self, notary_nodes: PublicKeys) {
        let _lock = self.acc_mtx.lock().unwrap();

        if let Some(ref curr_account) = self.curr_account {
            if notary_nodes.contains(&curr_account.public_key()) {
                return;
            }
        }

        let mut acc: Option<Arc<Account>> = None;
        for node in notary_nodes {
            acc = self.wallet.get_account(&node.get_script_hash());
            if let Some(ref account) = acc {
                if account.can_sign() {
                    break;
                }
                let err = account.decrypt(&self.config.main_cfg.unlock_wallet.password, &self.wallet.scrypt);
                if let Err(e) = err {
                    warn!(
                        "can't unlock notary node account: address={}, error={}",
                        address::uint160_to_string(&account.contract.script_hash()),
                        e
                    );
                    acc = None;
                }
                break;
            }
        }

        self.curr_account = acc.clone();
        if acc.is_none() {
            let _req_lock = self.req_mtx.lock().unwrap();
            let mut requests = self.requests.write().unwrap();
            *requests = HashMap::new();
        }
    }

    pub fn get_account(&self) -> Option<Arc<Account>> {
        let _lock = self.acc_mtx.lock().unwrap();
        self.curr_account.clone()
    }
}
