use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::mpsc::{self, Sender, Receiver};
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio::task;
use tokio::runtime::Runtime;
use reqwest::Client as HttpClient;
use reqwest::Url;
use anyhow::Result;
use uuid::Uuid;

use crate::core::block;
use crate::core::state;
use crate::core::transaction;
use crate::neorpc;
use crate::neorpc::result;
use crate::rpcclient;
use crate::rpcclient::actor;
use crate::rpcclient::waiter;
use crate::smartcontract;
use crate::smartcontract::trigger;
use crate::util;
use crate::vm::stackitem;

const DEFAULT_DIAL_TIMEOUT: Duration = Duration::from_secs(4);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

pub struct Client {
    cli: HttpClient,
    endpoint: Url,
    ctx: tokio::sync::Mutex<()>,
    ctx_cancel: tokio::sync::oneshot::Sender<()>,
    opts: Options,
    request_f: Box<dyn Fn(&neorpc::Request) -> Result<neorpc::Response, Box<dyn Error>> + Send + Sync>,
    reader: invoker::Invoker,
    cache_lock: RwLock<()>,
    cache: Cache,
    latest_req_id: AtomicU64,
    get_next_request_id: Box<dyn Fn() -> u64 + Send + Sync>,
}

pub struct Options {
    cert: String,
    key: String,
    cacert: String,
    dial_timeout: Duration,
    request_timeout: Duration,
    max_conns_per_host: usize,
}

pub struct Cache {
    init_done: bool,
    network: netmode::Magic,
    state_root_in_header: bool,
    native_hashes: HashMap<String, util::Uint160>,
}

impl Client {
    pub async fn new(ctx: tokio::sync::Mutex<()>, endpoint: &str, opts: Options) -> Result<Arc<Self>> {
        let mut cl = Client {
            cli: HttpClient::new(),
            endpoint: Url::parse(endpoint)?,
            ctx,
            ctx_cancel: tokio::sync::oneshot::channel().0,
            opts,
            request_f: Box::new(|_| Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "unimplemented")))),
            reader: invoker::Invoker::new(),
            cache_lock: RwLock::new(()),
            cache: Cache {
                init_done: false,
                network: netmode::Magic::MainNet,
                state_root_in_header: false,
                native_hashes: HashMap::new(),
            },
            latest_req_id: AtomicU64::new(0),
            get_next_request_id: Box::new(|| 0),
        };
        cl.init_client(endpoint, opts).await?;
        Ok(Arc::new(cl))
    }

    async fn init_client(&mut self, endpoint: &str, opts: Options) -> Result<()> {
        let url = Url::parse(endpoint)?;

        let http_client = HttpClient::builder()
            .timeout(opts.request_timeout)
            .build()?;

        self.ctx_cancel = tokio::sync::oneshot::channel().0;
        self.cli = http_client;
        self.endpoint = url;
        self.cache = Cache {
            native_hashes: HashMap::new(),
            ..self.cache
        };
        self.latest_req_id = AtomicU64::new(0);
        self.get_next_request_id = Box::new(|| self.latest_req_id.fetch_add(1, Ordering::SeqCst));
        self.opts = opts;
        self.request_f = Box::new(|r| self.make_http_request(r));
        self.reader = invoker::Invoker::new();
        Ok(())
    }

    fn get_request_id(&self) -> u64 {
        (self.get_next_request_id)()
    }

    pub async fn init(&self) -> Result<()> {
        let version = self.get_version().await?;
        let natives = self.get_native_contracts().await?;

        let mut cache_lock = self.cache_lock.write().unwrap();
        self.cache.network = version.protocol.network;
        self.cache.state_root_in_header = version.protocol.state_root_in_header;
        for ctr in natives {
            self.cache.native_hashes.insert(ctr.manifest.name, ctr.hash);
        }
        self.cache.init_done = true;
        Ok(())
    }

    pub fn close(&self) {
        self.ctx_cancel.send(()).unwrap();
    }

    pub async fn perform_request(&self, method: &str, p: Vec<serde_json::Value>, v: &mut serde_json::Value) -> Result<()> {
        let p = if p.is_empty() { vec![] } else { p };
        let r = neorpc::Request {
            jsonrpc: neorpc::JSONRPC_VERSION.to_string(),
            method: method.to_string(),
            params: p,
            id: self.get_request_id(),
        };

        let raw = (self.request_f)(&r)?;

        if let Some(err) = raw.error {
            return Err(Box::new(err));
        } else if raw.result.is_none() {
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "no result returned")));
        }
        *v = raw.result.unwrap();
        Ok(())
    }

    pub async fn make_http_request(&self, r: &neorpc::Request) -> Result<neorpc::Response, Box<dyn Error>> {
        let buf = serde_json::to_vec(r)?;
        let req = self.cli.post(self.endpoint.clone()).body(buf).build()?;
        let resp = self.cli.execute(req).await?;
        let raw: neorpc::Response = resp.json().await?;
        Ok(raw)
    }

    pub async fn ping(&self) -> Result<()> {
        let conn = tokio::net::TcpStream::connect(self.endpoint.host_str().unwrap()).await?;
        conn.shutdown().await?;
        Ok(())
    }

    pub fn context(&self) -> &tokio::sync::Mutex<()> {
        &self.ctx
    }

    pub fn endpoint(&self) -> &str {
        self.endpoint.as_str()
    }
}
