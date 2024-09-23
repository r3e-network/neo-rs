use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task;
use tokio::runtime::Runtime;
use anyhow::Result;
use crate::neorpc::{self, Notification, Request, Response};

pub type InternalHook = fn(tokio::sync::mpsc::Sender<neorpc::Notification>) -> Box<dyn Fn(Request) -> Result<Response> + Send + Sync>;

pub struct Internal {
    ws_client: WSClient,
    events: mpsc::Receiver<neorpc::Notification>,
}

impl Internal {
    pub async fn new(register: InternalHook) -> Result<Arc<Mutex<Self>>> {
        let (tx, rx) = mpsc::channel(100);
        let ws_client = WSClient::new().await?;
        let internal = Arc::new(Mutex::new(Internal {
            ws_client,
            events: rx,
        }));

        let request_f = register(tx);
        {
            let mut ws_client = internal.lock().unwrap();
            ws_client.ws_client.request_f = Some(request_f);
        }

        let internal_clone = Arc::clone(&internal);
        task::spawn(async move {
            internal_clone.lock().unwrap().event_loop().await;
        });

        Ok(internal)
    }

    async fn event_loop(&mut self) {
        loop {
            tokio::select! {
                _ = self.ws_client.ctx.done() => {
                    break;
                }
                Some(ev) = self.events.recv() => {
                    let ntf = Notification { 
                        event_type: ev.event_type, 
                        payload: ev.payload.clone() 
                    };
                    self.ws_client.notify_subscribers(ntf).await;
                }
            }
        }
    }
}

pub struct WSClient {
    client: Client,
    shutdown: oneshot::Sender<()>,
    reader_done: oneshot::Sender<()>,
    writer_done: oneshot::Sender<()>,
    subscriptions: HashMap<String, NotificationReceiver>,
    receivers: HashMap<String, Vec<String>>,
    request_f: Option<Box<dyn Fn(Request) -> Result<Response> + Send + Sync>>,
    ctx: tokio::sync::Mutex<()>,
    ctx_cancel: oneshot::Sender<()>,
}

impl WSClient {
    pub async fn new() -> Result<Self> {
        let (shutdown_tx, _) = oneshot::channel();
        let (reader_done_tx, _) = oneshot::channel();
        let (writer_done_tx, _) = oneshot::channel();
        let (ctx_cancel_tx, _) = oneshot::channel();

        Ok(WSClient {
            client: Client::new().await?,
            shutdown: shutdown_tx,
            reader_done: reader_done_tx,
            writer_done: writer_done_tx,
            subscriptions: HashMap::new(),
            receivers: HashMap::new(),
            request_f: None,
            ctx: tokio::sync::Mutex::new(()),
            ctx_cancel: ctx_cancel_tx,
        })
    }

    async fn notify_subscribers(&self, ntf: Notification) {
        // Implement the logic to notify subscribers
    }
}

pub struct Client;

impl Client {
    pub async fn new() -> Result<Self> {
        Ok(Client)
    }
}
