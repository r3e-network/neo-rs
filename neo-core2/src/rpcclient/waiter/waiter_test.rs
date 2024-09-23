use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use std::error::Error;
use std::fmt;
use std::sync::mpsc::{self, Sender, Receiver};
use uuid::Uuid;
use tokio::time::timeout;
use tokio::sync::Mutex;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::sync::oneshot;
use tokio::task;
use tokio::runtime::Runtime;

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
use anyhow::Result;

struct RPCClient {
    err: Option<anyhow::Error>,
    inv_res: Option<result::Invoke>,
    net_fee: i64,
    b_count: AtomicU32,
    version: Option<result::Version>,
    hash: Option<util::Uint256>,
    app_log: Option<result::ApplicationLog>,
    context: Option<tokio::sync::Mutex<tokio::sync::oneshot::Receiver<()>>>,
}

impl waiter::RPCPollingBased for RPCClient {}

#[async_trait::async_trait]
impl rpcclient::Client for RPCClient {
    async fn invoke_contract_verify(&self, contract: util::Uint160, params: Vec<smartcontract::Parameter>, signers: Vec<transaction::Signer>, witnesses: Vec<transaction::Witness>) -> Result<result::Invoke> {
        Ok(self.inv_res.clone().unwrap())
    }

    async fn invoke_function(&self, contract: util::Uint160, operation: String, params: Vec<smartcontract::Parameter>, signers: Vec<transaction::Signer>) -> Result<result::Invoke> {
        Ok(self.inv_res.clone().unwrap())
    }

    async fn invoke_script(&self, script: Vec<u8>, signers: Vec<transaction::Signer>) -> Result<result::Invoke> {
        Ok(self.inv_res.clone().unwrap())
    }

    async fn calculate_network_fee(&self, tx: transaction::Transaction) -> Result<i64> {
        Ok(self.net_fee)
    }

    async fn get_block_count(&self) -> Result<u32> {
        Ok(self.b_count.load(Ordering::SeqCst))
    }

    async fn get_version(&self) -> Result<result::Version> {
        Ok(self.version.clone().unwrap())
    }

    async fn send_raw_transaction(&self, tx: transaction::Transaction) -> Result<util::Uint256> {
        Ok(self.hash.clone().unwrap())
    }

    async fn terminate_session(&self, session_id: Uuid) -> Result<bool> {
        Ok(false) // Just a stub, unused by actor.
    }

    async fn traverse_iterator(&self, session_id: Uuid, iterator_id: Uuid, max_items_count: i32) -> Result<Vec<stackitem::Item>> {
        Ok(vec![]) // Just a stub, unused by actor.
    }

    async fn context(&self) -> tokio::sync::Mutex<tokio::sync::oneshot::Receiver<()>> {
        if self.context.is_none() {
            let (tx, rx) = oneshot::channel();
            self.context = Some(tokio::sync::Mutex::new(rx));
        }
        self.context.clone().unwrap()
    }

    async fn get_application_log(&self, hash: util::Uint256, trig: Option<trigger::Type>) -> Result<result::ApplicationLog> {
        if let Some(app_log) = &self.app_log {
            Ok(app_log.clone())
        } else {
            Err(anyhow::anyhow!("not found"))
        }
    }
}

struct AwaitableRPCClient {
    rpc_client: RPCClient,
    ch_lock: RwLock<()>,
    sub_header_ch: Option<tokio_mpsc::Sender<block::Header>>,
    sub_block_ch: Option<tokio_mpsc::Sender<block::Block>>,
    sub_tx_ch: Option<tokio_mpsc::Sender<state::AppExecResult>>,
}

impl waiter::RPCEventBased for AwaitableRPCClient {}

#[async_trait::async_trait]
impl rpcclient::Client for AwaitableRPCClient {
    async fn receive_blocks(&self, flt: neorpc::BlockFilter, rcvr: tokio_mpsc::Sender<block::Block>) -> Result<String> {
        let _lock = self.ch_lock.write().unwrap();
        self.sub_block_ch = Some(rcvr);
        Ok("1".to_string())
    }

    async fn receive_executions(&self, flt: neorpc::ExecutionFilter, rcvr: tokio_mpsc::Sender<state::AppExecResult>) -> Result<String> {
        let _lock = self.ch_lock.write().unwrap();
        self.sub_tx_ch = Some(rcvr);
        Ok("2".to_string())
    }

    async fn receive_headers_of_added_blocks(&self, flt: neorpc::BlockFilter, rcvr: tokio_mpsc::Sender<block::Header>) -> Result<String> {
        let _lock = self.ch_lock.write().unwrap();
        self.sub_header_ch = Some(rcvr);
        Ok("3".to_string())
    }

    async fn unsubscribe(&self, id: String) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn test_new_waiter() -> Result<()> {
    let w = waiter::new(None::<actor::RPCActor>, None);
    assert!(matches!(w, waiter::Null));

    let w = waiter::new(Some(RPCClient::default()), Some(result::Version::default()));
    assert!(matches!(w, waiter::PollingBased));

    let w = waiter::new(Some(AwaitableRPCClient::default()), Some(result::Version::default()));
    assert!(matches!(w, waiter::EventBased));

    Ok(())
}

#[tokio::test]
async fn test_polling_waiter_wait() -> Result<()> {
    let h = util::Uint256::from([1, 2, 3]);
    let b_count = 5;
    let app_log = result::ApplicationLog { container: h.clone(), executions: vec![state::Execution::default()] };
    let expected = state::AppExecResult { container: h.clone(), execution: state::Execution::default() };
    let c = RPCClient { app_log: Some(app_log), ..Default::default() };
    c.b_count.store(b_count, Ordering::SeqCst);
    let w = waiter::new(Some(c), Some(result::Version { protocol: result::Protocol { milliseconds_per_block: 1, ..Default::default() }, ..Default::default() })); // reduce testing time.
    assert!(matches!(w, waiter::PollingBased));

    // Wait with error.
    let some_err = anyhow::anyhow!("some error");
    let res = w.wait(h.clone(), b_count, Some(some_err.clone())).await;
    assert!(matches!(res, Err(e) if e == some_err));

    // AER is in chain immediately.
    let res = w.wait(h.clone(), b_count - 1, None).await;
    assert!(matches!(res, Ok(aer) if aer == expected));

    // Missing AER after VUB.
    c.app_log = None;
    let res = w.wait(h.clone(), b_count - 2, None).await;
    assert!(matches!(res, Err(e) if e == waiter::ErrTxNotAccepted));

    let check_err = |trigger: Box<dyn FnOnce() + Send>, target: anyhow::Error| async move {
        let (err_tx, err_rx) = oneshot::channel();
        let w_clone = w.clone();
        let h_clone = h.clone();
        let b_count_clone = b_count;
        task::spawn(async move {
            let res = w_clone.wait(h_clone, b_count_clone, None).await;
            err_tx.send(res.err().unwrap()).unwrap();
        });

        let mut timer = tokio::time::interval(Duration::from_secs(1));
        let mut trigger_fired = false;
        loop {
            tokio::select! {
                _ = timer.tick() => {
                    if trigger_fired {
                        panic!("failed to await result");
                    }
                    trigger();
                    trigger_fired = true;
                }
                res = err_rx => {
                    assert!(matches!(res.unwrap(), e if e == target));
                    break;
                }
            }
        }
        assert!(trigger_fired);
    };

    // Tx is accepted before VUB.
    c.app_log = None;
    c.b_count.store(b_count, Ordering::SeqCst);
    check_err(Box::new(|| c.b_count.store(b_count + 1, Ordering::SeqCst)), waiter::ErrTxNotAccepted).await;

    // Context is cancelled.
    c.app_log = None;
    c.b_count.store(b_count, Ordering::SeqCst);
    let (tx, rx) = oneshot::channel();
    c.context = Some(tokio::sync::Mutex::new(rx));
    check_err(Box::new(|| tx.send(()).unwrap()), waiter::ErrContextDone).await;
}

#[tokio::test]
async fn test_ws_waiter_wait() -> Result<()> {
    let h = util::Uint256::from([1, 2, 3]);
    let b_count = 5;
    let app_log = result::ApplicationLog { container: h.clone(), executions: vec![state::Execution::default()] };
    let expected = state::AppExecResult { container: h.clone(), execution: state::Execution::default() };
    let c = AwaitableRPCClient { rpc_client: RPCClient { app_log: Some(app_log), ..Default::default() }, ..Default::default() };
    c.rpc_client.b_count.store(b_count, Ordering::SeqCst);
    let w = waiter::new(Some(c), Some(result::Version { protocol: result::Protocol { milliseconds_per_block: 1, ..Default::default() }, ..Default::default() })); // reduce testing time.
    assert!(matches!(w, waiter::EventBased));

    // Wait with error.
    let some_err = anyhow::anyhow!("some error");
    let res = w.wait(h.clone(), b_count, Some(some_err.clone())).await;
    assert!(matches!(res, Err(e) if e == some_err));

    // AER is in chain immediately.
    let res = w.wait(h.clone(), b_count - 1, None).await;
    assert!(matches!(res, Ok(aer) if aer == expected));

    // Auxiliary things for asynchronous tests.
    let (done_tx, done_rx) = oneshot::channel();
    let check = |trigger: Box<dyn FnOnce() + Send>| async move {
        let mut timer = tokio::time::interval(Duration::from_secs(1));
        let mut trigger_fired = false;
        loop {
            tokio::select! {
                _ = timer.tick() => {
                    if trigger_fired {
                        panic!("failed to await result");
                    }
                    trigger();
                    trigger_fired = true;
                }
                _ = &mut done_rx => {
                    break;
                }
            }
        }
        assert!(trigger_fired);
    };

    // AER received after the subscription.
    c.rpc_client.app_log = None;
    let (aer_tx, aer_rx) = oneshot::channel();
    task::spawn(async move {
        let res = w.wait(h.clone(), b_count - 1, None).await;
        assert!(matches!(res, Ok(aer) if aer == expected));
        aer_tx.send(()).unwrap();
    });
    check(Box::new(|| {
        let _lock = c.ch_lock.read().unwrap();
        c.sub_tx_ch.as_ref().unwrap().try_send(expected.clone()).unwrap();
    })).await;

    // Missing AER after VUB.
    let (aer_tx, aer_rx) = oneshot::channel();
    task::spawn(async move {
        let res = w.wait(h.clone(), b_count - 2, None).await;
        assert!(matches!(res, Err(e) if e == waiter::ErrTxNotAccepted));
        aer_tx.send(()).unwrap();
    });
    check(Box::new(|| {
        let _lock = c.ch_lock.read().unwrap();
        c.sub_header_ch.as_ref().unwrap().try_send(block::Header::default()).unwrap();
    })).await;
}

#[tokio::test]
async fn test_rpc_waiter_rpc_client_compat() -> Result<()> {
    let _ = waiter::RPCPollingBased::new(rpcclient::Client::default());
    let _ = waiter::RPCPollingBased::new(rpcclient::WSClient::default());
    let _ = waiter::RPCEventBased::new(rpcclient::WSClient::default());
    Ok(())
}
