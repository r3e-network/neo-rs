use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::io::{self, Read};
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::thread;

use crate::core::fee;
use crate::core::interop;
use crate::core::native::nativehashes;
use crate::core::transaction;
use crate::crypto::hash;
use crate::crypto::keys;
use crate::smartcontract::callflag;
use crate::smartcontract::trigger;
use log::warn;
use log::debug;

pub struct Oracle {
    resp_mtx: Mutex<()>,
    responses: HashMap<u64, IncompleteTx>,
    removed: HashMap<u64, bool>,
    log: log::Logger,
    network: u32,
    oracle_info_lock: RwLock<()>,
    oracle_response: Vec<u8>,
    chain: Arc<Chain>,
    oracle_script: Vec<u8>,
    verify_offset: usize,
}

impl Oracle {
    fn get_response(&self, req_id: u64, create: bool) -> Option<Arc<Mutex<IncompleteTx>>> {
        let _lock = self.resp_mtx.lock().unwrap();
        if let Some(inc_tx) = self.responses.get(&req_id) {
            return Some(Arc::clone(inc_tx));
        }
        if create && !self.removed.contains_key(&req_id) {
            let inc_tx = Arc::new(Mutex::new(IncompleteTx::new()));
            self.responses.insert(req_id, Arc::clone(&inc_tx));
            return Some(inc_tx);
        }
        None
    }

    pub fn add_response(&self, pub_key: &keys::PublicKey, req_id: u64, tx_sig: &[u8]) {
        if let Some(inc_tx) = self.get_response(req_id, true) {
            let mut inc_tx = inc_tx.lock().unwrap();
            let mut is_backup = false;
            if let Some(tx) = &inc_tx.tx {
                if !pub_key.verify_hashable(tx_sig, self.network, tx) {
                    if !pub_key.verify_hashable(tx_sig, self.network, &inc_tx.backup_tx) {
                        debug!("invalid response signature", pub = pub_key.to_string_compressed());
                        return;
                    }
                    is_backup = true;
                }
            }
            inc_tx.add_response(pub_key, tx_sig, is_backup);
            let (ready_tx, ready) = inc_tx.finalize(self.get_oracle_nodes(), false);
            if ready && !inc_tx.is_sent {
                inc_tx.is_sent = true;
                self.send_tx(ready_tx);
            }
        }
    }

    pub fn read_response(&self, rc: &mut dyn Read, url: &str) -> Result<Vec<u8>, transaction::OracleResponseCode> {
        const LIMIT: usize = transaction::MAX_ORACLE_RESULT_SIZE;
        let mut buf = vec![0; LIMIT + 1];
        let n = match rc.read(&mut buf) {
            Ok(n) => n,
            Err(e) => return self.handle_response_error(None, Some(e), url),
        };
        if n <= LIMIT {
            match check_utf8(&buf[..n]) {
                Ok(res) => return self.handle_response_error(Some(res), None, url),
                Err(e) => return self.handle_response_error(None, Some(e), url),
            }
        }
        self.handle_response_error(None, Some(io::Error::new(io::ErrorKind::Other, "response too large")), url)
    }

    fn handle_response_error(&self, data: Option<Vec<u8>>, err: Option<io::Error>, url: &str) -> Result<Vec<u8>, transaction::OracleResponseCode> {
        if let Some(err) = err {
            warn!("failed to read data for oracle request", url = url, err = err);
            if err.kind() == io::ErrorKind::Other && err.to_string() == "response too large" {
                return Err(transaction::OracleResponseCode::ResponseTooLarge);
            }
            return Err(transaction::OracleResponseCode::Error);
        }
        Ok(data.unwrap())
    }

    pub fn create_response_tx(&self, gas_for_response: i64, vub: u32, resp: &transaction::OracleResponse) -> Result<transaction::Transaction, Box<dyn Error>> {
        let resp_script;
        {
            let _lock = self.oracle_info_lock.read().unwrap();
            resp_script = self.oracle_response.clone();
        }

        let mut tx = transaction::Transaction::new(resp_script, 0);
        tx.nonce = resp.id as u32;
        tx.valid_until_block = vub;
        tx.attributes.push(transaction::Attribute {
            attr_type: transaction::AttributeType::OracleResponse,
            value: transaction::AttributeValue::OracleResponse(resp.clone()),
        });

        let oracle_sign_contract = self.get_oracle_sign_contract();
        tx.signers.push(transaction::Signer {
            account: nativehashes::ORACLE_CONTRACT,
            scopes: transaction::WitnessScope::None,
        });
        tx.signers.push(transaction::Signer {
            account: hash::hash160(&oracle_sign_contract),
            scopes: transaction::WitnessScope::None,
        });
        tx.scripts.push(transaction::Witness::default());

        let size = tx.get_var_size();
        tx.scripts.push(transaction::Witness {
            verification_script: oracle_sign_contract.clone(),
            ..Default::default()
        });

        let (gas_consumed, ok, err) = self.test_verify(&tx)?;
        if !ok {
            return Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, "can't verify transaction")));
        }
        tx.network_fee += gas_consumed;

        let (net_fee, size_delta) = fee::calculate(self.chain.get_base_exec_fee(), &oracle_sign_contract);
        tx.network_fee += net_fee;
        let size = size + size_delta;

        let curr_net_fee = tx.network_fee + (size as i64) * self.chain.fee_per_byte();
        if curr_net_fee > gas_for_response {
            let attr_size = tx.get_var_size();
            resp.code = transaction::OracleResponseCode::InsufficientFunds;
            resp.result = None;
            let size = size - attr_size + tx.get_var_size();
        }
        tx.network_fee += (size as i64) * self.chain.fee_per_byte();

        tx.system_fee = gas_for_response - tx.network_fee;
        Ok(tx)
    }

    fn test_verify(&self, tx: &transaction::Transaction) -> Result<(i64, bool, Box<dyn Error>), Box<dyn Error>> {
        let mut cp = tx.clone();
        let ic = self.chain.get_test_vm(trigger::Verification, &mut cp, None)?;
        ic.vm.gas_limit = self.chain.get_max_verification_gas();

        {
            let _lock = self.oracle_info_lock.read().unwrap();
            ic.vm.load_script_with_hash(&self.oracle_script, nativehashes::ORACLE_CONTRACT, callflag::ReadOnly);
            ic.vm.context().jump(self.verify_offset);
        }

        let ok = is_verify_ok(&ic);
        Ok((ic.vm.gas_consumed(), ok, Box::new(fmt::Error::new(fmt::ErrorKind::Other, "failed to create test VM"))))
    }
}

fn check_utf8(v: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    if !std::str::from_utf8(v).is_ok() {
        return Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, "invalid UTF-8")));
    }
    Ok(v.to_vec())
}

fn is_verify_ok(ic: &interop::Context) -> bool {
    ic.finalize();
    if ic.vm.run().is_err() {
        return false;
    }
    if ic.vm.estack().len() != 1 {
        return false;
    }
    ic.vm.estack().pop().item().try_bool().unwrap_or(false)
}

fn get_failed_response(id: u64) -> transaction::OracleResponse {
    transaction::OracleResponse {
        id,
        code: transaction::OracleResponseCode::Error,
    }
}
