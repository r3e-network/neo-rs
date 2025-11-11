use std::time::SystemTime;

use rand::Rng;

use crate::{Capability, VersionPayload};

/// Utility for constructing a standard version payload for local node.
pub fn build_version_payload(
    network: u32,
    protocol: u32,
    user_agent: String,
    capabilities: Vec<Capability>,
) -> VersionPayload {
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as u32;
    let nonce = rand::thread_rng().gen::<u32>();
    VersionPayload::new(
        network,
        protocol,
        timestamp,
        nonce,
        if user_agent.is_empty() {
            format!("/neo-rs:{}", env!("CARGO_PKG_VERSION"))
        } else {
            user_agent
        },
        capabilities,
    )
}
