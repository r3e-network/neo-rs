use std::collections::HashMap;
use std::time::Duration;
use prometheus::{self, Encoder, Gauge, GaugeVec, Histogram, HistogramOpts, Opts, Registry};

lazy_static! {
    static ref ESTIMATED_NETWORK_SIZE: Gauge = Gauge::with_opts(Opts::new("network_size", "Estimated network size").namespace("neogo")).unwrap();
    static ref PEERS_CONNECTED: Gauge = Gauge::with_opts(Opts::new("peers_connected", "Number of connected peers").namespace("neogo")).unwrap();
    static ref SERV_AND_NODE_VERSION: GaugeVec = GaugeVec::new(Opts::new("serv_node_version", "Server and Node versions").namespace("neogo"), &["description", "value"]).unwrap();
    static ref NEOGO_VERSION: GaugeVec = GaugeVec::new(Opts::new("version", "NeoGo version").namespace("neogo"), &["version"]).unwrap();
    static ref SERVER_ID: GaugeVec = GaugeVec::new(Opts::new("server_id", "network server ID").namespace("neogo"), &["server_id"]).unwrap();
    static ref POOL_COUNT: Gauge = Gauge::with_opts(Opts::new("pool_count", "Number of available node addresses").namespace("neogo")).unwrap();
    static ref BLOCK_QUEUE_LENGTH: Gauge = Gauge::with_opts(Opts::new("block_queue_length", "Block queue length").namespace("neogo")).unwrap();
    static ref P2P_CMDS: HashMap<CommandType, Histogram> = {
        let mut map = HashMap::new();
        for cmd in &[CommandType::CMDVersion, CommandType::CMDVerack, CommandType::CMDGetAddr, CommandType::CMDAddr, CommandType::CMDPing, CommandType::CMDPong, CommandType::CMDGetHeaders, CommandType::CMDHeaders, CommandType::CMDGetBlocks, CommandType::CMDMempool, CommandType::CMDInv, CommandType::CMDGetData, CommandType::CMDGetBlockByIndex, CommandType::CMDNotFound, CommandType::CMDTX, CommandType::CMDBlock, CommandType::CMDExtensible, CommandType::CMDP2PNotaryRequest, CommandType::CMDGetMPTData, CommandType::CMDMPTData, CommandType::CMDReject, CommandType::CMDFilterLoad, CommandType::CMDFilterAdd, CommandType::CMDFilterClear, CommandType::CMDMerkleBlock, CommandType::CMDAlert] {
            let histogram = Histogram::with_opts(HistogramOpts::new(format!("p2p_{}_time", cmd.to_string().to_lowercase()), format!("P2P {} handling time", cmd.to_string())).namespace("neogo")).unwrap();
            map.insert(*cmd, histogram);
        }
        map
    };
    static ref NOTARYPOOL_UNSORTED_TX: Gauge = Gauge::with_opts(Opts::new("notarypool_unsorted_tx", "Notary request pool fallback txs").namespace("neogo")).unwrap();
}

pub fn init_metrics() {
    let registry = Registry::new();
    registry.register(Box::new(ESTIMATED_NETWORK_SIZE.clone())).unwrap();
    registry.register(Box::new(PEERS_CONNECTED.clone())).unwrap();
    registry.register(Box::new(SERV_AND_NODE_VERSION.clone())).unwrap();
    registry.register(Box::new(NEOGO_VERSION.clone())).unwrap();
    registry.register(Box::new(SERVER_ID.clone())).unwrap();
    registry.register(Box::new(POOL_COUNT.clone())).unwrap();
    registry.register(Box::new(BLOCK_QUEUE_LENGTH.clone())).unwrap();
    registry.register(Box::new(NOTARYPOOL_UNSORTED_TX.clone())).unwrap();
    for histogram in P2P_CMDS.values() {
        registry.register(Box::new(histogram.clone())).unwrap();
    }
}

pub fn update_network_size_metric(sz: i32) {
    ESTIMATED_NETWORK_SIZE.set(sz as f64);
}

pub fn update_block_queue_len_metric(bq_len: i32) {
    BLOCK_QUEUE_LENGTH.set(bq_len as f64);
}

pub fn update_pool_count_metric(p_count: i32) {
    POOL_COUNT.set(p_count as f64);
}

pub fn update_peers_connected_metric(p_connected: i32) {
    PEERS_CONNECTED.set(p_connected as f64);
}

// Deprecated: please, use set_neo_go_version and set_server_id instead.
pub fn set_server_and_node_versions(node_ver: &str, server_id: &str) {
    SERV_AND_NODE_VERSION.with_label_values(&["Node version: ", node_ver]).add(0.0);
    SERV_AND_NODE_VERSION.with_label_values(&["Server id: ", server_id]).add(0.0);
}

pub fn set_neo_go_version(node_ver: &str) {
    NEOGO_VERSION.with_label_values(&[node_ver]).add(1.0);
}

pub fn set_server_id(id: &str) {
    SERVER_ID.with_label_values(&[id]).add(1.0);
}

pub fn add_cmd_time_metric(cmd: CommandType, t: Duration) {
    if let Some(histogram) = P2P_CMDS.get(&cmd) {
        histogram.observe(t.as_secs_f64());
    }
}

// update_notarypool_metrics updates metric of the number of fallback txs inside
// the notary request pool.
pub fn update_notarypool_metrics(unsorted_txn_len: i32) {
    NOTARYPOOL_UNSORTED_TX.set(unsorted_txn_len as f64);
}
