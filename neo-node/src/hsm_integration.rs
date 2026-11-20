//! HSM CLI initialization glue.
//!
//! Phase 3 S6b: the hardware runtime and fail-closed initialization policy
//! sank into neo-hsm; only the NodeCli mapping stays in the assembly layer.

use crate::cli::NodeCli;
use anyhow::{Result, anyhow};
use neo_hsm::{ConsoleCredentialProvider, HsmConfig, HsmDeviceType, HsmRuntime};

pub use neo_hsm::print_hsm_status;

/// Maps the CLI flags onto the validated `neo-hsm` configuration.
fn config_from_cli(cli: &NodeCli) -> Result<HsmConfig> {
    let device_type: HsmDeviceType = cli
        .hsm_device
        .parse()
        .map_err(|error: String| anyhow!(error))?;
    Ok(HsmConfig {
        device_type,
        slot: cli.hsm_slot,
        key_id: cli.hsm_key_id.clone(),
        pkcs11_lib: cli.hsm_pkcs11_lib.clone(),
        skip_pin: cli.hsm_no_pin,
        ..HsmConfig::default()
    })
}

/// Initializes an HSM from command-line configuration.
///
/// Phase 9 (F15): the CLI is the adapter that owns terminal interaction, so the
/// console credential provider is injected here; `neo-hsm` itself no longer
/// reads stdin.
pub async fn initialize_hsm(cli: &NodeCli, address_version: u8) -> Result<HsmRuntime> {
    initialize_hsm_with_credentials(cli, address_version, &ConsoleCredentialProvider).await
}

/// Initializes an HSM while supplying credentials from outside the terminal.
///
/// Library-style embeddings (tests, remote orchestrators, future CLI frontends)
/// pass their own provider instead of inheriting the console behavior above.
pub async fn initialize_hsm_with_credentials(
    cli: &NodeCli,
    address_version: u8,
    credentials: &dyn neo_hsm::CredentialProvider,
) -> Result<HsmRuntime> {
    let config = config_from_cli(cli)?;
    neo_hsm::initialize_with_config(config, address_version, credentials).await
}
