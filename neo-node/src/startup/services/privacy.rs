use anyhow::{Context, Result};
use neo_rpc::server::RpcServer;
use std::sync::Arc;
use tracing::info;

/// Registers the opt-in encrypted submission RPC endpoint (`sendrawtransactionencrypted`)
/// when the given TEE runtime has privacy mode enabled.
///
/// Strictly opt-in: with privacy off (or no runtime at all) nothing is
/// registered and the RPC method surface is unchanged. Returns whether the
/// endpoint was installed.
#[cfg(feature = "tee")]
pub(crate) fn install_privacy_rpc_endpoint(
    server: &mut RpcServer,
    runtime: Option<&Arc<crate::tee_integration::TeeRuntime>>,
) -> Result<bool> {
    let Some(runtime) = runtime else {
        return Ok(false);
    };
    if !runtime.privacy_enabled() {
        return Ok(false);
    }

    let submitter = crate::tee_integration::privacy_submitter(runtime).context(
        "privacy mode is enabled but the encrypted submission submitter could not be built",
    )?;
    server.set_privacy_submitter(Some(submitter));
    server.register_handlers(neo_rpc::server::RpcServerPrivacy::register_handlers());
    info!(
        target: "neo::rpc",
        "privacy mode: sendrawtransactionencrypted endpoint registered"
    );
    Ok(true)
}
