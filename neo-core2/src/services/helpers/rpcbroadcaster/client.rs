use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use log::{error, Logger};
use crate::rpcclient::{Client, Options};

pub struct RPCClient {
    client: Option<Client>,
    addr: String,
    close: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    responses: Receiver<Vec<any>>,
    log: Arc<Logger>,
    send_timeout: Duration,
    method: SendMethod,
}

pub type SendMethod = fn(&Client, Vec<any>) -> Result<(), Box<dyn std::error::Error>>;

impl RPCClient {
    pub fn new(addr: String, method: SendMethod, timeout: Duration, ch: Receiver<Vec<any>>, log: Arc<Logger>, close: Arc<AtomicBool>) -> Self {
        RPCClient {
            client: None,
            addr,
            close,
            finished: Arc::new(AtomicBool::new(false)),
            responses: ch,
            log: log.clone(),
            send_timeout: timeout,
            method,
        }
    }

    pub fn run(&mut self) {
        let addr = self.addr.clone();
        let send_timeout = self.send_timeout;
        let log = self.log.clone();
        let close = self.close.clone();
        let finished = self.finished.clone();
        let responses = self.responses.clone();
        let method = self.method;

        thread::spawn(move || {
            let mut client = Client::new(&addr, Options {
                dial_timeout: send_timeout,
                request_timeout: send_timeout,
            }).ok();

            'run: loop {
                if close.load(Ordering::SeqCst) {
                    break 'run;
                }

                match responses.recv() {
                    Ok(ps) => {
                        if client.is_none() {
                            client = Client::new(&addr, Options {
                                dial_timeout: send_timeout,
                                request_timeout: send_timeout,
                            }).ok();

                            if client.is_none() {
                                error!(log, "failed to create client to submit oracle response");
                                continue;
                            }
                        }

                        if let Err(err) = method(client.as_ref().unwrap(), ps) {
                            error!(log, "error while submitting oracle response: {:?}", err);
                        }
                    }
                    Err(_) => break 'run,
                }
            }

            if let Some(client) = client {
                client.close();
            }

            'drain: loop {
                match responses.try_recv() {
                    Ok(_) => {}
                    Err(mpsc::TryRecvError::Empty) => break 'drain,
                    Err(mpsc::TryRecvError::Disconnected) => break 'drain,
                }
            }

            finished.store(true, Ordering::SeqCst);
        });
    }
}
