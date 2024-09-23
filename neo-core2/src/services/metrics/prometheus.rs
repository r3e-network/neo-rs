use std::net::SocketAddr;
use std::sync::Arc;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use hyper::rt::Future;
use tokio::sync::Mutex;
use tokio::runtime::Runtime;
use crate::config::BasicService;
use crate::logger::Logger;
use prometheus::{Encoder, TextEncoder, gather};
use prometheus::register_counter;
use prometheus::opts;

pub struct PrometheusService {
    name: String,
    servers: Vec<Server<SocketAddr>>,
    config: BasicService,
    log: Arc<Logger>,
}

impl PrometheusService {
    pub fn new(cfg: BasicService, log: Arc<Logger>) -> Option<Self> {
        if log.is_none() {
            return None;
        }

        let addrs = cfg.addresses;
        let mut srvs = Vec::with_capacity(addrs.len());
        for addr in addrs {
            let server = Server::bind(&addr.parse().unwrap())
                .serve(make_service_fn(|_| {
                    async {
                        Ok::<_, hyper::Error>(service_fn(|_req: Request<Body>| {
                            async {
                                let encoder = TextEncoder::new();
                                let metric_families = gather();
                                let mut buffer = Vec::new();
                                encoder.encode(&metric_families, &mut buffer).unwrap();
                                Ok::<_, hyper::Error>(Response::new(Body::from(buffer)))
                            }
                        }))
                    }
                }));
            srvs.push(server);
        }

        Some(PrometheusService {
            name: "Prometheus".to_string(),
            servers: srvs,
            config: cfg,
            log,
        })
    }
}
