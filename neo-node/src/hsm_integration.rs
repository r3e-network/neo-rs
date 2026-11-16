//! HSM CLI initialization glue.
//!
//! Phase 3 S6b: the hardware runtime and fail-closed initialization policy
//! sank into neo-hsm; only the NodeCli mapping stays in the assembly layer.

use crate::cli::NodeCli;
use anyhow::{Result, anyhow};
use neo_hsm::{HsmConfig, HsmDeviceType, HsmRuntime};

pub use neo_hsm::print_hsm_status;

/// Initializes an HSM from command-line configuration.
pub async fn initialize_hsm(cli: &NodeCli, address_version: u8) -> Result<HsmRuntime> {
    let device_type: HsmDeviceType = cli
        .hsm_device
        .parse()
        .map_err(|error: String| anyhow!(error))?;
    let config = HsmConfig {
        device_type,
        slot: cli.hsm_slot,
        key_id: cli.hsm_key_id.clone(),
        pkcs11_lib: cli.hsm_pkcs11_lib.clone(),
        skip_pin: cli.hsm_no_pin,
        ..HsmConfig::default()
    };
    neo_hsm::initialize_with_config(config, address_version).await
}
