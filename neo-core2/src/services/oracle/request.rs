use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::error::Error;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::TryRecvError;

use reqwest::blocking::Client;
use reqwest::header::{CONTENT_TYPE, USER_AGENT};
use url::Url;
use log::{debug, warn};
use mime::Mime;

use crate::core::state::OracleRequest;
use crate::core::storage;
use crate::core::transaction::{self, OracleResponse};
use crate::crypto::keys::PrivateKey;
use crate::services::oracle::neofs;
use crate::Chain;

const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 10;

struct Request {
    id: u64,
    req: Option<OracleRequest>,
}

struct Oracle {
    close: Arc<AtomicBool>,
    request_ch: Receiver<Request>,
    resp_mtx: Arc<Mutex<()>>,
    pending: Arc<Mutex<HashMap<u64, OracleRequest>>>,
    responses: Arc<Mutex<HashMap<u64, OracleRequest>>>,
    request_map: Sender<HashMap<u64, OracleRequest>>,
    client: Client,
    main_cfg: MainConfig,
    chain: Arc<Chain>,
    network: u32,
    response_handler: ResponseHandler,
}

impl Oracle {
    fn run_request_worker(&self) {
        let close = Arc::clone(&self.close);
        let request_ch = self.request_ch.clone();
        let resp_mtx = Arc::clone(&self.resp_mtx);
        let pending = Arc::clone(&self.pending);
        let responses = Arc::clone(&self.responses);
        let request_map = self.request_map.clone();
        let client = self.client.clone();
        let main_cfg = self.main_cfg.clone();
        let chain = Arc::clone(&self.chain);
        let network = self.network;
        let response_handler = self.response_handler.clone();

        thread::spawn(move || {
            while !close.load(Ordering::SeqCst) {
                match request_ch.try_recv() {
                    Ok(req) => {
                        let acc = self.get_account();
                        if acc.is_none() {
                            continue;
                        }
                        let acc = acc.unwrap();
                        if let Err(err) = self.process_request(&acc.private_key(), req) {
                            debug!("can't process request: {:?}", err);
                        }
                    }
                    Err(TryRecvError::Empty) => {
                        thread::sleep(Duration::from_millis(100));
                    }
                    Err(TryRecvError::Disconnected) => {
                        break;
                    }
                }
            }
        });
    }

    fn remove_requests(&self, ids: Vec<u64>) {
        let mut resp_mtx = self.resp_mtx.lock().unwrap();
        if !self.running {
            let mut pending = self.pending.lock().unwrap();
            for id in ids {
                pending.remove(&id);
            }
        } else {
            let mut responses = self.responses.lock().unwrap();
            for id in ids {
                responses.remove(&id);
            }
        }
    }

    fn add_requests(&self, reqs: HashMap<u64, OracleRequest>) {
        if reqs.is_empty() {
            return;
        }

        let mut resp_mtx = self.resp_mtx.lock().unwrap();
        if !self.running {
            let mut pending = self.pending.lock().unwrap();
            for (id, r) in reqs {
                pending.insert(id, r);
            }
            return;
        }

        drop(resp_mtx);

        match self.request_map.send(reqs) {
            Ok(_) => {}
            Err(_) => {
                let mut old_reqs = self.request_map.recv().unwrap();
                for (id, r) in old_reqs.drain() {
                    reqs.insert(id, r);
                }
                self.request_map.send(reqs).unwrap();
            }
        }
    }

    fn process_requests_internal(&self, reqs: HashMap<u64, OracleRequest>) {
        let acc = self.get_account();
        if acc.is_none() {
            return;
        }
        let acc = acc.unwrap();

        for (id, req) in reqs {
            if let Err(err) = self.process_request(&acc.private_key(), Request { id, req: Some(req) }) {
                debug!("can't process request: {:?}", err);
            }
        }
    }

    fn process_request(&self, priv_key: &PrivateKey, req: Request) -> Result<(), Box<dyn Error>> {
        if req.req.is_none() {
            self.process_failed_request(priv_key, req);
            return Ok(());
        }

        let inc_tx = self.get_response(req.id, true);
        if inc_tx.is_none() {
            return Ok(());
        }
        let inc_tx = inc_tx.unwrap();

        let mut resp = OracleResponse { id: req.id, code: transaction::SUCCESS, result: vec![] };
        let url = Url::parse(&req.req.as_ref().unwrap().url)?;
        match url.scheme() {
            "https" => {
                let http_req = self.client.get(req.req.as_ref().unwrap().url.clone())
                    .header(USER_AGENT, "NeoOracleService/3.0")
                    .header(CONTENT_TYPE, "application/json")
                    .build()?;
                let r = self.client.execute(http_req)?;
                match r.status() {
                    reqwest::StatusCode::OK => {
                        if !check_media_type(r.headers().get(CONTENT_TYPE).unwrap().to_str()?, &self.main_cfg.allowed_content_types) {
                            resp.code = transaction::CONTENT_TYPE_NOT_SUPPORTED;
                        } else {
                            let mut body = vec![];
                            r.bytes().read_to_end(&mut body)?;
                            resp.result = body;
                        }
                    }
                    reqwest::StatusCode::FORBIDDEN => resp.code = transaction::FORBIDDEN,
                    reqwest::StatusCode::NOT_FOUND => resp.code = transaction::NOT_FOUND,
                    reqwest::StatusCode::REQUEST_TIMEOUT => resp.code = transaction::TIMEOUT,
                    _ => resp.code = transaction::ERROR,
                }
            }
            neofs::URI_SCHEME => {
                if self.main_cfg.neofs.nodes.is_empty() {
                    warn!("no NeoFS nodes configured: {}", req.req.as_ref().unwrap().url);
                    resp.code = transaction::ERROR;
                } else {
                    let ctx = context::Context::new();
                    let index = (req.id as usize + inc_tx.attempts) % self.main_cfg.neofs.nodes.len();
                    let rc = neofs::get(ctx, priv_key, &url, &self.main_cfg.neofs.nodes[index])?;
                    let mut body = vec![];
                    rc.read_to_end(&mut body)?;
                    resp.result = body;
                }
            }
            _ => {
                resp.code = transaction::PROTOCOL_NOT_SUPPORTED;
                warn!("unknown oracle request scheme: {}", req.req.as_ref().unwrap().url);
            }
        }

        if resp.code == transaction::SUCCESS {
            resp.result = filter_request(resp.result, req.req.as_ref().unwrap())?;
        }

        debug!("oracle request processed: {} code: {} result: {:?}", req.req.as_ref().unwrap().url, resp.code, String::from_utf8_lossy(&resp.result));

        let current_height = self.chain.block_height();
        let vub_inc = self.chain.get_config().max_valid_until_block_increment;
        let (_, h) = self.chain.get_transaction(&req.req.as_ref().unwrap().original_tx_id)?;
        let h = h.unwrap_or(current_height) + vub_inc;
        let tx = self.create_response_tx(req.req.as_ref().unwrap().gas_for_response as i64, h, &resp)?;
        let backup_tx = self.create_response_tx(req.req.as_ref().unwrap().gas_for_response as i64, h + vub_inc, &OracleResponse {
            id: req.id,
            code: transaction::CONSENSUS_UNREACHABLE,
            result: vec![],
        })?;

        inc_tx.lock();
        inc_tx.request = req.req;
        inc_tx.tx = Some(tx.clone());
        inc_tx.backup_tx = Some(backup_tx.clone());
        inc_tx.reverify_tx(self.network);

        let tx_sig = priv_key.sign_hashable(self.network, &tx);
        inc_tx.add_response(priv_key.public_key(), tx_sig.clone(), false);

        let backup_sig = priv_key.sign_hashable(self.network, &backup_tx);
        inc_tx.add_response(priv_key.public_key(), backup_sig.clone(), true);

        let (ready_tx, ready) = inc_tx.finalize(self.get_oracle_nodes(), false);
        if ready {
            inc_tx.is_sent = true;
        }
        inc_tx.time = std::time::SystemTime::now();
        inc_tx.attempts += 1;
        inc_tx.unlock();

        self.response_handler.send_response(priv_key, &resp, &tx_sig);
        if ready {
            self.send_tx(&ready_tx);
        }
        Ok(())
    }

    fn process_failed_request(&self, priv_key: &PrivateKey, req: Request) {
        let inc_tx = self.get_response(req.id, false);
        if inc_tx.is_none() {
            return;
        }
        let inc_tx = inc_tx.unwrap();
        if inc_tx.is_sent {
            self.send_tx(&inc_tx.tx);
            return;
        }

        inc_tx.lock();
        let (ready_tx, ready) = inc_tx.finalize(self.get_oracle_nodes(), true);
        if ready {
            inc_tx.is_sent = true;
        }
        inc_tx.time = std::time::SystemTime::now();
        inc_tx.attempts += 1;
        let tx_sig = inc_tx.backup_sigs.get(&priv_key.public_key().to_bytes()).unwrap().sig.clone();
        inc_tx.unlock();

        self.response_handler.send_response(priv_key, &get_failed_response(req.id), &tx_sig);
        if ready {
            self.send_tx(&ready_tx);
        }
    }
}

fn check_media_type(hdr: &str, allowed: &[String]) -> bool {
    if allowed.is_empty() {
        return true;
    }

    let typ: Mime = hdr.parse().unwrap();
    allowed.contains(&typ.to_string())
}
