// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::time::Duration;

#[allow(unused_imports)]
use duration_str::deserialize_duration;
use serde::{Deserialize, Serialize};
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

use neo_p2p::{LocalNode, MessageHandleV2, P2pConfig};


#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Config {
    #[serde(default = "graceful", deserialize_with = "deserialize_duration")]
    pub graceful: Duration,

    #[serde(default)]
    pub p2p: P2pConfig,
}


pub const fn graceful() -> Duration {
    Duration::from_secs(10)
}


#[derive(clap::Args)]
pub(crate) struct NodeCmd {
    #[arg(long, help = "The node config file path")]
    pub config: String,
}


pub(crate) fn run_node(config: Config) -> anyhow::Result<()> {
    let local = LocalNode::new(config.p2p);
    let p2p_config = local.p2p_config();
    let handle = MessageHandleV2::new(
        local.port(),
        p2p_config.clone(),
        local.net_handles(),
    );

    let node = local.run(handle);
    log::info!("run as node on {}, pid {}", p2p_config.listen.as_str(), std::process::id());

    let mut signals = Signals::new(&[SIGHUP, SIGINT, SIGTERM])?;
    for signal in &mut signals {
        log::warn!("node exiting(wait {:?}), because signal '{}' received.", config.graceful, signal);
        break;
    };
    drop(node);

    // wait for gracefully exit
    std::thread::sleep(config.graceful);
    log::warn!("node exited, pid {}", std::process::id());

    Ok(())
}