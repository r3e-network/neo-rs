use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use std::time::Duration;
use log::error;

pub struct RPCBroadcaster {
    clients: HashMap<String, Arc<Mutex<RPCClient>>>,
    log: Arc<Mutex<zap::Logger>>,
    responses: Sender<Vec<any>>,
    close: Sender<()>,
    finished: Receiver<()>,
    send_timeout: Duration,
}

impl RPCBroadcaster {
    pub fn new(log: Arc<Mutex<zap::Logger>>, send_timeout: Duration) -> Self {
        let (close_tx, close_rx) = channel();
        let (finished_tx, finished_rx) = channel();
        let (responses_tx, responses_rx) = channel();

        RPCBroadcaster {
            clients: HashMap::new(),
            log,
            responses: responses_tx,
            close: close_tx,
            finished: finished_rx,
            send_timeout,
        }
    }

    pub fn run(&self) {
        for client in self.clients.values() {
            let client = Arc::clone(client);
            thread::spawn(move || {
                client.lock().unwrap().run();
            });
        }

        loop {
            select! {
                recv(self.close) -> _ => break,
                recv(self.responses) -> ps => {
                    if let Ok(ps) = ps {
                        for client in self.clients.values() {
                            let client = Arc::clone(client);
                            let ps = ps.clone();
                            thread::spawn(move || {
                                let client = client.lock().unwrap();
                                if let Err(_) = client.responses.send(ps) {
                                    error!("can't send response, channel is full");
                                }
                            });
                        }
                    }
                }
            }
        }

        for client in self.clients.values() {
            let client = Arc::clone(client);
            thread::spawn(move || {
                let _ = client.lock().unwrap().finished.recv();
            });
        }

        loop {
            select! {
                recv(self.responses) -> _ => {},
                default => break,
            }
        }
    }

    pub fn send_params(&self, params: Vec<any>) {
        select! {
            recv(self.close) -> _ => {},
            send(self.responses, params) -> _ => {},
        }
    }

    pub fn shutdown(&self) {
        let _ = self.close.send(());
        let _ = self.finished.recv();
    }
}
