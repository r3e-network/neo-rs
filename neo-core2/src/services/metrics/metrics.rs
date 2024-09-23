use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::net::TcpListener;
use std::thread;
use std::error::Error;
use std::fmt;
use tokio::sync::Mutex;
use tokio::net::TcpListener as TokioTcpListener;
use tokio::task;
use hyper::server::Server;
use hyper::service::make_service_fn;
use hyper::service::service_fn;
use hyper::Body;
use hyper::Request;
use hyper::Response;
use hyper::Error as HyperError;
use log::{info, error};
use crate::config::BasicService;

pub struct Service {
    http: Vec<Arc<Mutex<Server<TokioTcpListener>>>>,
    config: BasicService,
    log: slog::Logger,
    service_type: String,
    started: AtomicBool,
}

impl Service {
    pub fn new(name: String, http_servers: Vec<Arc<Mutex<Server<TokioTcpListener>>>>, cfg: BasicService, log: slog::Logger) -> Self {
        Service {
            http: http_servers,
            config: cfg,
            service_type: name.clone(),
            log: log.new(slog::o!("service" => name)),
            started: AtomicBool::new(false),
        }
    }

    pub async fn start(&self) -> Result<(), Box<dyn Error>> {
        if self.config.enabled {
            if !self.started.compare_and_swap(false, true, Ordering::SeqCst) {
                info!(self.log, "service already started");
                return Ok(());
            }
            for srv in &self.http {
                let srv = srv.clone();
                let log = self.log.clone();
                let addr = srv.lock().await.local_addr();
                info!(log, "starting service"; "endpoint" => addr.to_string());

                let listener = TcpListener::bind(addr)?;
                let addr = listener.local_addr()?;
                srv.lock().await.local_addr = addr;

                task::spawn(async move {
                    let server = srv.lock().await;
                    if let Err(e) = server.serve_with_incoming(listener.incoming()).await {
                        if e != HyperError::from(hyper::Error::new_canceled()) {
                            error!(log, "failed to start service"; "endpoint" => addr.to_string(), "error" => e.to_string());
                        }
                    }
                });
            }
        } else {
            info!(self.log, "service hasn't started since it's disabled");
        }
        Ok(())
    }

    pub async fn shutdown(&self) {
        if !self.config.enabled {
            return;
        }
        if !self.started.compare_and_swap(true, false, Ordering::SeqCst) {
            return;
        }
        for srv in &self.http {
            let srv = srv.clone();
            let log = self.log.clone();
            let addr = srv.lock().await.local_addr();
            info!(log, "shutting down service"; "endpoint" => addr.to_string());
            if let Err(e) = srv.lock().await.shutdown().await {
                error!(log, "can't shut service down"; "endpoint" => addr.to_string(), "error" => e.to_string());
            }
        }
        let _ = self.log.flush();
    }
}
