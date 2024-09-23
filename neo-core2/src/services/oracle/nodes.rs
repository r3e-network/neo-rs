use std::sync::{Arc, RwLock};
use std::sync::Mutex;
use crate::crypto::keys::PublicKeys;
use crate::encoding::address;
use crate::smartcontract;
use crate::wallet;
use log::error;
use log::info;

pub struct Oracle {
    acc_mtx: RwLock<()>,
    oracle_nodes: PublicKeys,
    curr_account: Option<wallet::Account>,
    oracle_sign_contract: Vec<u8>,
    wallet: Arc<wallet::Wallet>,
    main_cfg: MainConfig,
    log: Arc<log::Logger>,
}

impl Oracle {
    // UpdateOracleNodes updates oracle nodes list.
    pub fn update_oracle_nodes(&self, oracle_nodes: PublicKeys) {
        let _lock = self.acc_mtx.write().unwrap();

        if self.oracle_nodes == oracle_nodes {
            return;
        }

        let mut acc: Option<wallet::Account> = None;
        for node in oracle_nodes.iter() {
            acc = self.wallet.get_account(&node.get_script_hash());
            if let Some(ref account) = acc {
                if account.can_sign() {
                    break;
                }
                if let Err(err) = account.decrypt(&self.main_cfg.unlock_wallet.password, &self.wallet.scrypt) {
                    self.log.error(&format!("can't unlock account: address={}, error={}", 
                        address::uint160_to_string(&account.contract.script_hash()), err));
                    self.curr_account = None;
                    return;
                }
                break;
            }
        }

        self.curr_account = acc;
        self.oracle_sign_contract = smartcontract::create_default_multi_sig_redeem_script(&oracle_nodes).unwrap();
        self.oracle_nodes = oracle_nodes;
    }

    pub fn get_account(&self) -> Option<wallet::Account> {
        let _lock = self.acc_mtx.read().unwrap();
        self.curr_account.clone()
    }

    pub fn get_oracle_nodes(&self) -> PublicKeys {
        let _lock = self.acc_mtx.read().unwrap();
        self.oracle_nodes.clone()
    }

    pub fn get_oracle_sign_contract(&self) -> Vec<u8> {
        let _lock = self.acc_mtx.read().unwrap();
        self.oracle_sign_contract.clone()
    }
}
