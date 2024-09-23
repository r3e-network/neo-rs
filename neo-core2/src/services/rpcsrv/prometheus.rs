use std::collections::HashMap;
use std::time::Duration;
use prometheus::{Histogram, HistogramOpts, Registry};
use lazy_static::lazy_static;

lazy_static! {
    static ref RPC_TIMES: HashMap<String, Histogram> = HashMap::new();
}

pub fn add_req_time_metric(name: &str, t: Duration) {
    if let Some(hist) = RPC_TIMES.get(name) {
        hist.observe(t.as_secs_f64());
    }
}

fn reg_counter(call: &str, registry: &Registry) {
    let histogram = Histogram::with_opts(HistogramOpts::new(
        format!("rpc_{}_time", call.to_lowercase()),
        format!("RPC {} call handling time", call),
    )
    .namespace("neogo"))
    .unwrap();
    
    registry.register(Box::new(histogram.clone())).unwrap();
    RPC_TIMES.insert(call.to_string(), histogram);
}

pub fn init(rpc_handlers: &HashMap<String, ()>, rpc_ws_handlers: &HashMap<String, ()>, registry: &Registry) {
    for call in rpc_handlers.keys() {
        reg_counter(call, registry);
    }
    for call in rpc_ws_handlers.keys() {
        reg_counter(call, registry);
    }
}
