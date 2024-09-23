use std::net::SocketAddr;
use std::sync::Arc;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use hyper::rt::Future;
use tokio::sync::Mutex;
use tokio::runtime::Runtime;
use crate::config::BasicService;
use crate::logger::Logger;

pub struct PprofService {
    name: String,
    servers: Vec<Server<SocketAddr>>,
    config: BasicService,
    log: Arc<Mutex<Logger>>,
}

impl PprofService {
    pub fn new(cfg: BasicService, log: Arc<Mutex<Logger>>) -> Option<Self> {
        if log.lock().await.is_none() {
            return None;
        }

        let handler = make_service_fn(|_| {
            async {
                Ok::<_, hyper::Error>(service_fn(|req: Request<Body>| {
                    async move {
                        match req.uri().path() {
                            "/debug/pprof/" => pprof_index(req).await,
                            "/debug/pprof/cmdline" => pprof_cmdline(req).await,
                            "/debug/pprof/profile" => pprof_profile(req).await,
                            "/debug/pprof/symbol" => pprof_symbol(req).await,
                            "/debug/pprof/trace" => pprof_trace(req).await,
                            _ => Ok(Response::new(Body::from("Not Found"))),
                        }
                    }
                }))
            }
        });

        let addrs = cfg.addresses;
        let mut servers = Vec::with_capacity(addrs.len());
        for addr in addrs {
            let server = Server::bind(&addr.parse::<SocketAddr>().unwrap())
                .serve(handler.clone());
            servers.push(server);
        }

        Some(PprofService {
            name: "Pprof".to_string(),
            servers,
            config: cfg,
            log,
        })
    }
}

async fn pprof_index(_req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    // Implement the pprof index handler
    Ok(Response::new(Body::from("pprof index")))
}

async fn pprof_cmdline(_req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    // Implement the pprof cmdline handler
    Ok(Response::new(Body::from("pprof cmdline")))
}

async fn pprof_profile(_req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    // Implement the pprof profile handler
    Ok(Response::new(Body::from("pprof profile")))
}

async fn pprof_symbol(_req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    // Implement the pprof symbol handler
    Ok(Response::new(Body::from("pprof symbol")))
}

async fn pprof_trace(_req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    // Implement the pprof trace handler
    Ok(Response::new(Body::from("pprof trace")))
}
