use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{error::Error, fmt, thread};

use tokio::sync::mpsc;
use tokio::time::interval;

use crate::core::block::{Block, Header};
use crate::core::state::{AppExecResult, Execution};
use crate::neorpc::{self, result, trigger};
use crate::util::Uint256;

const DEFAULT_POLL_RETRY_COUNT: usize = 3;

#[derive(Debug, Clone)]
pub struct WaiterError {
    pub message: String,
}

impl fmt::Display for WaiterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for WaiterError {}

impl WaiterError {
    pub fn new(message: &str) -> Self {
        WaiterError {
            message: message.to_string(),
        }
    }
}

pub trait Waiter {
    fn wait(&self, h: Uint256, vub: u32, err: Option<Box<dyn Error>>) -> Result<AppExecResult, Box<dyn Error>>;
    fn wait_any(&self, ctx: &tokio::sync::mpsc::Receiver<()>, vub: u32, hashes: Vec<Uint256>) -> Result<AppExecResult, Box<dyn Error>>;
}

pub trait RPCPollingBased {
    fn context(&self) -> &tokio::sync::mpsc::Receiver<()>;
    fn get_version(&self) -> Result<result::Version, Box<dyn Error>>;
    fn get_block_count(&self) -> Result<u32, Box<dyn Error>>;
    fn get_application_log(&self, hash: Uint256, trig: &trigger::Type) -> Result<result::ApplicationLog, Box<dyn Error>>;
}

pub trait RPCEventBased: RPCPollingBased {
    fn receive_headers_of_added_blocks(&self, flt: &neorpc::BlockFilter, rcvr: mpsc::Sender<Header>) -> Result<String, Box<dyn Error>>;
    fn receive_blocks(&self, flt: &neorpc::BlockFilter, rcvr: mpsc::Sender<Block>) -> Result<String, Box<dyn Error>>;
    fn receive_executions(&self, flt: &neorpc::ExecutionFilter, rcvr: mpsc::Sender<AppExecResult>) -> Result<String, Box<dyn Error>>;
    fn unsubscribe(&self, id: &str) -> Result<(), Box<dyn Error>>;
}

pub struct Null;

pub struct PollingBased {
    polling: Arc<dyn RPCPollingBased + Send + Sync>,
    version: result::Version,
    config: PollConfig,
}

pub struct Config {
    pub poll_config: PollConfig,
}

pub struct PollConfig {
    pub poll_interval: Duration,
    pub retry_count: usize,
}

pub struct EventBased {
    ws: Arc<dyn RPCEventBased + Send + Sync>,
    polling: Arc<dyn Waiter + Send + Sync>,
}

impl Null {
    pub fn new() -> Self {
        Null
    }
}

impl Waiter for Null {
    fn wait(&self, _h: Uint256, _vub: u32, _err: Option<Box<dyn Error>>) -> Result<AppExecResult, Box<dyn Error>> {
        Err(Box::new(WaiterError::new("awaiting not supported")))
    }

    fn wait_any(&self, _ctx: &tokio::sync::mpsc::Receiver<()>, _vub: u32, _hashes: Vec<Uint256>) -> Result<AppExecResult, Box<dyn Error>> {
        Err(Box::new(WaiterError::new("awaiting not supported")))
    }
}

impl PollingBased {
    pub fn new(waiter: Arc<dyn RPCPollingBased + Send + Sync>) -> Result<Self, Box<dyn Error>> {
        Self::new_custom(waiter, PollConfig {
            poll_interval: Duration::from_millis(500),
            retry_count: DEFAULT_POLL_RETRY_COUNT,
        })
    }

    pub fn new_custom(waiter: Arc<dyn RPCPollingBased + Send + Sync>, config: PollConfig) -> Result<Self, Box<dyn Error>> {
        let version = waiter.get_version()?;
        Ok(PollingBased {
            polling: waiter,
            version,
            config,
        })
    }
}

impl Waiter for PollingBased {
    fn wait(&self, h: Uint256, vub: u32, err: Option<Box<dyn Error>>) -> Result<AppExecResult, Box<dyn Error>> {
        if let Some(e) = err {
            if !e.to_string().to_lowercase().contains("already exists") {
                return Err(e);
            }
        }
        self.wait_any(&tokio::sync::mpsc::channel(1).1, vub, vec![h])
    }

    fn wait_any(&self, ctx: &tokio::sync::mpsc::Receiver<()>, vub: u32, hashes: Vec<Uint256>) -> Result<AppExecResult, Box<dyn Error>> {
        let mut current_height = 0;
        let mut failed_attempt = 0;
        let mut interval = interval(self.config.poll_interval);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let block_count = self.polling.get_block_count()?;
                    if block_count - 1 > current_height {
                        current_height = block_count - 1;
                    }
                    let trig = trigger::Type::Application;
                    for h in &hashes {
                        if let Ok(res) = self.polling.get_application_log(*h, &trig) {
                            return Ok(AppExecResult {
                                container: res.container,
                                execution: res.executions[0].clone(),
                            });
                        }
                    }
                    if current_height >= vub {
                        return Err(Box::new(WaiterError::new("transaction was not accepted to chain")));
                    }
                }
                _ = self.polling.context().recv() => {
                    return Err(Box::new(WaiterError::new("waiter context done")));
                }
                _ = ctx.recv() => {
                    return Err(Box::new(WaiterError::new("waiter context done")));
                }
            }
        }
    }
}

impl EventBased {
    pub fn new(waiter: Arc<dyn RPCEventBased + Send + Sync>) -> Result<Self, Box<dyn Error>> {
        Self::new_custom(waiter, Config {
            poll_config: PollConfig {
                poll_interval: Duration::from_millis(500),
                retry_count: DEFAULT_POLL_RETRY_COUNT,
            },
        })
    }

    pub fn new_custom(waiter: Arc<dyn RPCEventBased + Send + Sync>, config: Config) -> Result<Self, Box<dyn Error>> {
        let polling = Arc::new(PollingBased::new_custom(waiter.clone(), config.poll_config)?);
        Ok(EventBased {
            ws: waiter,
            polling,
        })
    }
}

impl Waiter for EventBased {
    fn wait(&self, h: Uint256, vub: u32, err: Option<Box<dyn Error>>) -> Result<AppExecResult, Box<dyn Error>> {
        if let Some(e) = err {
            if !e.to_string().to_lowercase().contains("already exists") {
                return Err(e);
            }
        }
        self.wait_any(&tokio::sync::mpsc::channel(1).1, vub, vec![h])
    }

    fn wait_any(&self, ctx: &tokio::sync::mpsc::Receiver<()>, vub: u32, hashes: Vec<Uint256>) -> Result<AppExecResult, Box<dyn Error>> {
        let (h_rcvr_tx, mut h_rcvr_rx) = mpsc::channel(2);
        let (b_rcvr_tx, mut b_rcvr_rx) = mpsc::channel(2);
        let (aer_rcvr_tx, mut aer_rcvr_rx) = mpsc::channel(hashes.len());
        let (unsub_errs_tx, mut unsub_errs_rx) = mpsc::channel(1);
        let (exit_tx, exit_rx) = mpsc::channel(1);

        let mut ws_wait_err: Option<Box<dyn Error>> = None;
        let mut waiters_active = 0;

        let since = vub;
        let blocks_id = match self.ws.receive_headers_of_added_blocks(&neorpc::BlockFilter { since: Some(since) }, h_rcvr_tx.clone()) {
            Ok(id) => id,
            Err(err) => {
                if err.to_string().contains("invalid params") {
                    self.ws.receive_blocks(&neorpc::BlockFilter { since: Some(since) }, b_rcvr_tx.clone()).unwrap_or_else(|e| {
                        ws_wait_err = Some(Box::new(e));
                        String::new()
                    })
                } else {
                    ws_wait_err = Some(Box::new(err));
                    String::new()
                }
            }
        };

        if ws_wait_err.is_none() {
            waiters_active += 1;
            let ws = self.ws.clone();
            let exit_tx = exit_tx.clone();
            tokio::spawn(async move {
                exit_rx.recv().await;
                if let Err(err) = ws.unsubscribe(&blocks_id).await {
                    unsub_errs_tx.send(Box::new(WaiterError::new(&format!("failed to unsubscribe from blocks/headers (id: {}): {}", blocks_id, err)))).await.unwrap();
                } else {
                    unsub_errs_tx.send(Ok(())).await.unwrap();
                }
            });

            let trig = trigger::Type::Application;
            for h in &hashes {
                let txs_id = match self.ws.receive_executions(&neorpc::ExecutionFilter { container: Some(*h) }, aer_rcvr_tx.clone()) {
                    Ok(id) => id,
                    Err(err) => {
                        ws_wait_err = Some(Box::new(WaiterError::new(&format!("failed to subscribe for execution results: {}", err))));
                        break;
                    }
                };

                waiters_active += 1;
                let ws = self.ws.clone();
                let exit_tx = exit_tx.clone();
                tokio::spawn(async move {
                    exit_rx.recv().await;
                    if let Err(err) = ws.unsubscribe(&txs_id).await {
                        unsub_errs_tx.send(Box::new(WaiterError::new(&format!("failed to unsubscribe from transactions (id: {}): {}", txs_id, err)))).await.unwrap();
                    } else {
                        unsub_errs_tx.send(Ok(())).await.unwrap();
                    }
                });

                if let Ok(app_log) = self.ws.get_application_log(*h, &trig) {
                    return Ok(AppExecResult {
                        container: app_log.container,
                        execution: app_log.executions[0].clone(),
                    });
                }
            }
        }

        if ws_wait_err.is_none() {
            tokio::select! {
                Some(_) = h_rcvr_rx.recv() => {
                    return Err(Box::new(WaiterError::new("transaction was not accepted to chain")));
                }
                Some(_) = b_rcvr_rx.recv() => {
                    return Err(Box::new(WaiterError::new("transaction was not accepted to chain")));
                }
                Some(aer) = aer_rcvr_rx.recv() => {
                    return Ok(aer);
                }
                _ = self.ws.context().recv() => {
                    return Err(Box::new(WaiterError::new("waiter context done")));
                }
                _ = ctx.recv() => {
                    return Err(Box::new(WaiterError::new("waiter context done")));
                }
            }
        }

        exit_tx.send(()).await.unwrap();

        while waiters_active > 0 {
            tokio::select! {
                Some(_) = h_rcvr_rx.recv() => {}
                Some(_) = b_rcvr_rx.recv() => {}
                Some(_) = aer_rcvr_rx.recv() => {}
                Some(unsub_err) = unsub_errs_rx.recv() => {
                    if let Err(err) = unsub_err {
                        ws_wait_err = Some(Box::new(WaiterError::new(&format!("unsubscription error: {}", err))));
                    }
                    waiters_active -= 1;
                }
            }
        }

        if let Some(err) = ws_wait_err {
            return Err(err);
        }

        self.polling.wait_any(ctx, vub, hashes)
    }
}
