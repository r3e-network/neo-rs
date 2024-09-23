use std::collections::HashMap;
use std::sync::{Arc, RwLock, Mutex};
use std::time::{Duration, Instant};
use std::thread;
use std::sync::mpsc::{self, Sender, Receiver};

use bytes::Bytes;
use log::Logger;
use reqwest::blocking::Client;
use slices::Contains;
use tokio::sync::RwLock as AsyncRwLock;

use crate::config::{self, netmode};
use crate::core::{block, interop, state, transaction};
use crate::crypto::keys;
use crate::services::oracle::broadcaster;
use crate::smartcontract::trigger;
use crate::util;
use crate::wallet;

pub trait Ledger {
    fn block_height(&self) -> u32;
    fn fee_per_byte(&self) -> i64;
    fn get_base_exec_fee(&self) -> i64;
    fn get_config(&self) -> config::Blockchain;
    fn get_max_verification_gas(&self) -> i64;
    fn get_test_vm(&self, t: trigger::Type, tx: &transaction::Transaction, b: &block::Block) -> Result<interop::Context, Box<dyn std::error::Error>>;
    fn get_transaction(&self, hash: util::Uint256) -> Result<(transaction::Transaction, u32), Box<dyn std::error::Error>>;
}

pub struct Oracle {
    config: Config,
    oracle_info_lock: RwLock<()>,
    oracle_response: Vec<u8>,
    oracle_script: Vec<u8>,
    verify_offset: i32,
    acc_mtx: RwLock<()>,
    curr_account: Option<wallet::Account>,
    oracle_nodes: keys::PublicKeys,
    oracle_sign_contract: Vec<u8>,
    close: Sender<()>,
    done: Receiver<()>,
    request_ch: Sender<Request>,
    request_map: Arc<Mutex<HashMap<u64, state::OracleRequest>>>,
    resp_mtx: RwLock<()>,
    running: bool,
    pending: HashMap<u64, state::OracleRequest>,
    responses: HashMap<u64, IncompleteTx>,
    removed: HashMap<u64, bool>,
    wallet: wallet::Wallet,
}

pub struct Config {
    log: Arc<Logger>,
    network: netmode::Magic,
    main_cfg: config::OracleConfiguration,
    client: Arc<Client>,
    chain: Arc<dyn Ledger>,
    response_handler: Arc<dyn Broadcaster>,
    on_transaction: Arc<dyn Fn(&transaction::Transaction) -> Result<(), Box<dyn std::error::Error>> + Send + Sync>,
}

pub trait HTTPClient {
    fn do_request(&self, req: &http::Request) -> Result<http::Response, Box<dyn std::error::Error>>;
}

pub trait Broadcaster {
    fn send_response(&self, priv_key: &keys::PrivateKey, resp: &transaction::OracleResponse, tx_sig: &[u8]);
    fn run(&self);
    fn shutdown(&self);
}

pub type TxCallback = dyn Fn(&transaction::Transaction) -> Result<(), Box<dyn std::error::Error>> + Send + Sync;

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_TASK_TIMEOUT: Duration = Duration::from_secs(3600);
const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(180);
const MAX_REDIRECTIONS: u32 = 2;

#[derive(Debug)]
pub struct OracleError;

impl std::fmt::Display for OracleError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "oracle request redirection error")
    }
}

impl std::error::Error for OracleError {}

impl Oracle {
    pub fn new(cfg: Config) -> Result<Self, Box<dyn std::error::Error>> {
        let (close_tx, close_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let (request_tx, request_rx) = mpsc::channel();

        let mut o = Oracle {
            config: cfg,
            oracle_info_lock: RwLock::new(()),
            oracle_response: Vec::new(),
            oracle_script: Vec::new(),
            verify_offset: 0,
            acc_mtx: RwLock::new(()),
            curr_account: None,
            oracle_nodes: keys::PublicKeys::new(),
            oracle_sign_contract: Vec::new(),
            close: close_tx,
            done: done_rx,
            request_ch: request_tx,
            request_map: Arc::new(Mutex::new(HashMap::new())),
            resp_mtx: RwLock::new(()),
            running: false,
            pending: HashMap::new(),
            responses: HashMap::new(),
            removed: HashMap::new(),
            wallet: wallet::Wallet::new(),
        };

        if o.config.main_cfg.request_timeout == Duration::new(0, 0) {
            o.config.main_cfg.request_timeout = DEFAULT_REQUEST_TIMEOUT;
        }
        if o.config.main_cfg.neofs.timeout == Duration::new(0, 0) {
            o.config.main_cfg.neofs.timeout = DEFAULT_REQUEST_TIMEOUT;
        }
        if o.config.main_cfg.max_concurrent_requests == 0 {
            o.config.main_cfg.max_concurrent_requests = DEFAULT_MAX_CONCURRENT_REQUESTS;
        }
        o.request_ch = request_tx.clone();
        if o.config.main_cfg.max_task_timeout == Duration::new(0, 0) {
            o.config.main_cfg.max_task_timeout = DEFAULT_MAX_TASK_TIMEOUT;
        }
        if o.config.main_cfg.refresh_interval == Duration::new(0, 0) {
            o.config.main_cfg.refresh_interval = DEFAULT_REFRESH_INTERVAL;
        }

        let w = &o.config.main_cfg.unlock_wallet;
        o.wallet = wallet::Wallet::new_from_file(&w.path)?;

        let have_account = o.wallet.accounts.iter().any(|acc| acc.decrypt(&w.password, &o.wallet.scrypt).is_ok());
        if !have_account {
            return Err(Box::new(OracleError));
        }

        if o.config.response_handler.is_none() {
            o.config.response_handler = Some(Arc::new(broadcaster::Broadcaster::new(o.config.main_cfg.clone(), o.config.log.clone())));
        }
        if o.config.on_transaction.is_none() {
            o.config.on_transaction = Some(Arc::new(|_tx| Ok(())));
        }
        if o.config.client.is_none() {
            o.config.client = Some(Arc::new(Client::new()));
        }

        Ok(o)
    }

    pub fn name(&self) -> &str {
        "oracle"
    }

    pub fn shutdown(&self) {
        let _ = self.resp_mtx.write().unwrap();
        if !self.running {
            return;
        }
        self.config.log.info("stopping oracle service");
        self.running = false;
        let _ = self.close.send(());
        self.config.response_handler.shutdown();
        let _ = self.done.recv();
        self.wallet.close();
        let _ = self.config.log.sync();
    }

    pub fn start(&self) {
        let _ = self.resp_mtx.write().unwrap();
        if self.running {
            return;
        }
        self.config.log.info("starting oracle service");
        let o = self.clone();
        thread::spawn(move || o.start_internal());
    }

    pub fn is_authorized(&self) -> bool {
        self.get_account().is_some()
    }

    fn start_internal(&self) {
        let _ = self.request_map.lock().unwrap().insert(self.pending.clone());
        self.pending.clear();
        self.running = true;
        let _ = self.resp_mtx.write().unwrap();

        for _ in 0..self.config.main_cfg.max_concurrent_requests {
            let o = self.clone();
            thread::spawn(move || o.run_request_worker());
        }
        let o = self.clone();
        thread::spawn(move || o.config.response_handler.run());

        let tick = tokio::time::interval(self.config.main_cfg.refresh_interval);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = self.close.recv() => break,
                    _ = tick.tick() => {
                        let mut reprocess = Vec::new();
                        let _ = self.resp_mtx.write().unwrap();
                        self.removed.clear();
                        for (id, inc_tx) in &self.responses {
                            let since = Instant::now().duration_since(inc_tx.time);
                            if since > self.config.main_cfg.max_task_timeout {
                                self.removed.insert(*id, true);
                            } else if since > self.config.main_cfg.refresh_interval {
                                reprocess.push(*id);
                            }
                        }
                        for id in &self.removed {
                            self.responses.remove(id);
                        }
                        let _ = self.resp_mtx.write().unwrap();

                        for id in reprocess {
                            let _ = self.request_ch.send(Request { id, req: None });
                        }
                    },
                    reqs = self.request_map.recv() => {
                        for (id, req) in reqs {
                            let _ = self.request_ch.send(Request { id, req: Some(req) });
                        }
                    }
                }
            }
        });
    }

    pub fn update_native_contract(&self, script: &[u8], resp: &[u8], h: util::Uint160, verify_offset: i32) {
        let _ = self.oracle_info_lock.write().unwrap();
        self.oracle_script = script.to_vec();
        self.oracle_response = resp.to_vec();
        self.verify_offset = verify_offset;
    }

    pub fn send_tx(&self, tx: &transaction::Transaction) {
        if let Err(err) = (self.config.on_transaction)(tx) {
            self.config.log.error(&format!("can't pool oracle tx: {}", err));
        }
    }
}
