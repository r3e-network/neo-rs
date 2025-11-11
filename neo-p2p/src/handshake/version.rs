use std::time::SystemTime;

use rand::Rng;

use crate::message::{Endpoint, VersionPayload};

/// Utility for constructing a standard version payload for local node.
pub fn build_version_payload(
    network: u32,
    protocol: u32,
    services: u64,
    receiver: Endpoint,
    sender: Endpoint,
    start_height: u32,
) -> VersionPayload {
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let nonce = rand::thread_rng().gen::<u64>();
    VersionPayload::new(
        network,
        protocol,
        services,
        timestamp,
        receiver,
        sender,
        nonce,
        format!("/neo-rs:{}", env!("CARGO_PKG_VERSION")),
        start_height,
        true,
    )
}
