use neo_config::ProtocolSettings;
use neo_consensus::ValidatorInfo;
use serde::Deserialize;

use super::ConsensusSetup;

#[cfg(feature = "hsm")]
use super::resolve_public_key_index;
#[cfg(feature = "hsm")]
use neo_crypto::ECPoint;
#[cfg(feature = "hsm")]
use neo_primitives::hex_util;
#[cfg(feature = "hsm")]
use std::sync::Arc;
#[cfg(feature = "hsm")]
use tracing::{info, warn};

/// `[consensus.hsm]`: HSM-backed consensus signing over PKCS#11.
///
/// The PIN is never stored in the TOML; it is read at startup from the
/// `pin_env` environment variable (default `NEO_HSM_CU_PASSWORD`). Requires
/// the node to be built with `--features hsm`.
//
// The fields below are deserialized from the `[consensus.hsm]` TOML table in
// every build, but they are only *read* by the `hsm` feature's signer code.
// Without that feature enabled they have no consumers, so we silence the
// dead-code lint: the struct is the configuration schema, and dropping a
// field just because the feature is off would silently discard user config.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct HsmKeyConfig {
    /// `aws` | `azure-cloud-hsm` | `azure-dedicated-hsm` | `gcp-cloud-hsm` | `generic`.
    pub provider: String,
    /// PKCS#11 `.so` to load; defaults to the provider's library when omitted.
    #[serde(default)]
    pub library_path: Option<String>,
    /// PKCS#11 slot number; first slot with a token present when omitted.
    #[serde(default)]
    pub slot: Option<u64>,
    /// Token label to match when `slot` is omitted.
    #[serde(default)]
    pub token_label: Option<String>,
    /// `CKA_LABEL` of the consensus private key.
    pub key_label: String,
    /// Optional `CKA_ID` (hex) to disambiguate keys sharing a label.
    #[serde(default)]
    pub key_id_hex: Option<String>,
    /// Environment variable holding the `C_Login` PIN.
    #[serde(default = "default_hsm_pin_env")]
    pub pin_env: String,
}

fn default_hsm_pin_env() -> String {
    "NEO_HSM_CU_PASSWORD".to_string()
}

/// Connects to the configured HSM, derives this node's validator index from the
/// HSM public key, and returns a setup whose signing is HSM-backed (the software
/// `private_key` is left zeroed and unused).
#[cfg(feature = "hsm")]
pub(super) fn build_hsm_consensus_setup(
    settings: &ProtocolSettings,
    validators: Vec<ValidatorInfo>,
    cfg: &HsmKeyConfig,
) -> anyhow::Result<Option<ConsensusSetup>> {
    use std::path::PathBuf;

    let provider = match cfg.provider.to_ascii_lowercase().as_str() {
        "aws" => neo_hsm::HsmProvider::Aws,
        "azure-cloud-hsm" | "azure" => neo_hsm::HsmProvider::AzureCloudHsm,
        "azure-dedicated-hsm" => neo_hsm::HsmProvider::AzureDedicatedHsm,
        "gcp-cloud-hsm" | "gcp" => neo_hsm::HsmProvider::GcpCloudHsm,
        "yubihsm2" | "yubihsm" => neo_hsm::HsmProvider::YubiHsm2,
        "nshield" => neo_hsm::HsmProvider::NShield,
        "softhsm2" | "softhsm" => neo_hsm::HsmProvider::SoftHsm2,
        "utimaco" => neo_hsm::HsmProvider::Utimaco,
        "generic" | "generic-pkcs11" => neo_hsm::HsmProvider::GenericPkcs11,
        other => anyhow::bail!("[consensus.hsm].provider {other:?} is not recognized"),
    };
    let library_path = cfg
        .library_path
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(neo_hsm::profile(provider).default_library));
    let key_id = match &cfg.key_id_hex {
        Some(h) => Some(
            hex_util::decode_hex(h.trim())
                .map_err(|e| anyhow::anyhow!("[consensus.hsm].key_id_hex is not valid hex: {e}"))?,
        ),
        None => None,
    };
    let user_pin = std::env::var(&cfg.pin_env)
        .map_err(|_| anyhow::anyhow!("[consensus.hsm] PIN env var {} is not set", cfg.pin_env))?;

    let hsm_config = neo_hsm::HsmConfig {
        provider,
        library_path,
        slot: cfg.slot,
        token_label: cfg.token_label.clone(),
        key_label: cfg.key_label.clone(),
        key_id,
        user_pin,
    };
    let signer = neo_hsm::Pkcs11Signer::connect(&hsm_config)
        .map_err(|e| anyhow::anyhow!("HSM connect failed: {e}"))?;
    let public_key = ECPoint::from_bytes(signer.public_key())
        .map_err(|e| anyhow::anyhow!("HSM public key is not a valid secp256r1 point: {e}"))?;
    let my_index = resolve_public_key_index(&public_key, &validators);
    if my_index.is_none() {
        warn!(
            pubkey = %hex_util::encode_hex(signer.public_key()),
            "HSM consensus key is not in the validator set; the node will relay only"
        );
    }
    info!(provider = ?cfg.provider, "HSM consensus signer connected");
    Ok(Some(ConsensusSetup {
        validators,
        public_key,
        my_index,
        private_key: [0u8; 32],
        network: settings.network,
        ms_per_block: u64::from(settings.milliseconds_per_block),
        signer: Some(Arc::new(signer)),
    }))
}

#[cfg(not(feature = "hsm"))]
pub(super) fn build_hsm_consensus_setup(
    _settings: &ProtocolSettings,
    _validators: Vec<ValidatorInfo>,
    _cfg: &HsmKeyConfig,
) -> anyhow::Result<Option<ConsensusSetup>> {
    anyhow::bail!(
        "[consensus.hsm] is configured but neo-node was built without the `hsm` feature; rebuild with --features hsm"
    )
}
