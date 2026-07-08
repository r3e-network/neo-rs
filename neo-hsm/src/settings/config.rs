//! HSM provider configuration.
//!
//! Defines [`HsmProvider`] (selects the cloud vendor), [`HsmConfig`] (runtime
//! parameters), [`SigFormat`] (how to post-process raw HSM output), and
//! [`ProviderProfile`] (the per-provider constant table).

use std::path::PathBuf;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::errors::{HsmError, HsmResult};

/// Selects the HSM provider (and thus the `.so` path defaults, login model,
/// key-id scheme, and signature-format post-processing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HsmProvider {
    /// AWS CloudHSM (Cavium/Marvell SDK5, `libcloudhsm_pkcs11.so`).
    ///
    /// Login: `"<CU_user>:<password>"` AUTH pin.
    /// Sig format: raw 64-byte `r‖s` — no DER decode needed.
    Aws,

    /// Azure Cloud HSM GA (general-purpose PKCS#11, `libazcloudhsm_pkcs11.so`).
    ///
    /// Login: `"<CU_user>:<password>"` AUTH pin.
    /// Sig format: raw 64-byte `r‖s` — no DER decode needed.
    AzureCloudHsm,

    /// Azure Dedicated HSM (Luna 7 partition, Chrystoki `libCryptoki2_64.so`).
    ///
    /// Login: partition-user PIN.
    /// Sig format: raw 64-byte `r‖s` — no DER decode needed.
    AzureDedicatedHsm,

    /// GCP Cloud KMS via `libkmsp11.so` (kms-integrations v1.9).
    ///
    /// Login: ADC / Workload Identity / SA JSON via env — the `C_Login` call is
    /// issued but the pin is not validated by the library; set to empty string.
    /// Sig format: DER / ASN.1 `SEQUENCE { INTEGER r, INTEGER s }` — must be
    /// decoded to raw `r‖s` before returning.
    GcpCloudHsm,

    /// YubiHSM 2 via `yubihsm_pkcs11.so`. Sig format: raw `r‖s`.
    YubiHsm2,

    /// Entrust (Thales) nShield via `libcknfast.so`. Sig format: raw `r‖s`.
    NShield,

    /// SoftHSM2 (`libsofthsm2.so`) — software PKCS#11 for dev/testing the HSM
    /// path without hardware. Sig format: raw `r‖s`.
    SoftHsm2,

    /// Utimaco SecurityServer via `libcs_pkcs11_R3.so`. Sig format: raw `r‖s`.
    Utimaco,

    /// Any other PKCS#11 HSM (Generic Luna, nCipher, etc.).
    ///
    /// The caller supplies the full profile via [`HsmConfig`].
    GenericPkcs11,
}

/// How to post-process the raw bytes returned by `C_Sign`.
///
/// After decoding, both paths run through low-s normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigFormat {
    /// The HSM returns a raw 64-byte `r‖s` big-endian pair (AWS, Azure paths).
    RawRs,

    /// The HSM returns a DER / ASN.1 `SEQUENCE { INTEGER r, INTEGER s }` (GCP
    /// PKCS#11 path via `libkmsp11`). Must be decoded to 64-byte `r‖s`.
    Der,
}

/// Static per-provider constants.
#[derive(Debug, Clone)]
pub struct ProviderProfile {
    /// Default `.so` path to `dlopen` when the config does not specify one.
    pub default_library: &'static str,
    /// How the raw `C_Sign` output should be post-processed.
    pub signature_format: SigFormat,
}

/// Returns the [`ProviderProfile`] for a given [`HsmProvider`].
///
/// The caller may override `library_path` in [`HsmConfig`]; the profile
/// only supplies the default and the invariant signature-format.
#[must_use]
pub fn profile(provider: HsmProvider) -> ProviderProfile {
    match provider {
        HsmProvider::Aws => ProviderProfile {
            default_library: "/opt/cloudhsm/lib/libcloudhsm_pkcs11.so",
            signature_format: SigFormat::RawRs,
        },
        HsmProvider::AzureCloudHsm => ProviderProfile {
            default_library: "/opt/azurecloudhsm/lib/libazcloudhsm_pkcs11.so",
            signature_format: SigFormat::RawRs,
        },
        HsmProvider::AzureDedicatedHsm => ProviderProfile {
            // Luna 7 / Chrystoki client — exact path depends on deployment.
            default_library: "/usr/lib/libCryptoki2_64.so",
            signature_format: SigFormat::RawRs,
        },
        HsmProvider::GcpCloudHsm => ProviderProfile {
            default_library: "/opt/kmsp11/libkmsp11.so",
            // GCP's libkmsp11 returns DER-encoded signatures, not raw r||s.
            signature_format: SigFormat::Der,
        },
        HsmProvider::YubiHsm2 => ProviderProfile {
            default_library: "/usr/lib/pkcs11/yubihsm_pkcs11.so",
            signature_format: SigFormat::RawRs,
        },
        HsmProvider::NShield => ProviderProfile {
            default_library: "/opt/nfast/toolkits/pkcs11/libcknfast.so",
            signature_format: SigFormat::RawRs,
        },
        HsmProvider::SoftHsm2 => ProviderProfile {
            default_library: "/usr/lib/softhsm/libsofthsm2.so",
            signature_format: SigFormat::RawRs,
        },
        HsmProvider::Utimaco => ProviderProfile {
            default_library: "/opt/utimaco/lib/libcs_pkcs11_R3.so",
            signature_format: SigFormat::RawRs,
        },
        HsmProvider::GenericPkcs11 => ProviderProfile {
            default_library: "/usr/lib/libpkcs11.so",
            signature_format: SigFormat::RawRs,
        },
    }
}

/// Runtime configuration for the PKCS#11 signer.
///
/// Secrets (PIN) are held in a zeroize-on-drop wrapper and must be sourced
/// from the environment (`NEO_HSM_CU_PASSWORD`) — never from a TOML file.
#[derive(Debug, Clone, Zeroize, ZeroizeOnDrop)]
pub struct HsmConfig {
    /// Cloud provider / HSM type. Drives the default `.so` path and sig format.
    #[zeroize(skip)]
    pub provider: HsmProvider,

    /// Path to the PKCS#11 `.so` to `dlopen`.  Overrides the provider default.
    #[zeroize(skip)]
    pub library_path: PathBuf,

    /// Slot index (PKCS#11 slot number).  When `None` the first slot that has a
    /// token present (`C_GetSlotList(CK_TRUE)`) is used.
    #[zeroize(skip)]
    pub slot: Option<u64>,

    /// Token label to match when `slot` is `None`.  When both are `None` the
    /// first available slot is used.
    #[zeroize(skip)]
    pub token_label: Option<String>,

    /// `CKA_LABEL` value used to locate the private key (and matching public key).
    #[zeroize(skip)]
    pub key_label: String,

    /// Optional `CKA_ID` (hex or raw) to narrow the key search when multiple
    /// keys share the same label.
    #[zeroize(skip)]
    pub key_id: Option<Vec<u8>>,

    /// PIN passed to `C_Login(CKU_USER, pin)`.
    ///
    /// For AWS/Azure Cloud HSM: `"<CU_user>:<password>"`.
    /// For GCP libkmsp11: an empty string (credentials come from ADC env).
    /// The field is zeroized on drop.
    pub user_pin: String,
}

impl HsmConfig {
    /// Build a config from environment for AWS CloudHSM.
    ///
    /// Reads `NEO_HSM_CU_PASSWORD` from the environment and returns an
    /// initialization error when it is absent.
    pub fn from_env_aws(
        cu_user: impl Into<String>,
        key_label: impl Into<String>,
    ) -> HsmResult<Self> {
        Self::from_env_aws_with(cu_user, key_label, |name| std::env::var(name))
    }

    fn from_env_aws_with(
        cu_user: impl Into<String>,
        key_label: impl Into<String>,
        read_env: impl FnOnce(&str) -> Result<String, std::env::VarError>,
    ) -> HsmResult<Self> {
        let cu_password = read_env("NEO_HSM_CU_PASSWORD").map_err(|_| {
            HsmError::Init("NEO_HSM_CU_PASSWORD must be set for AWS CloudHSM".to_string())
        })?;
        Ok(Self::aws_cloudhsm(cu_user, key_label, cu_password))
    }

    fn aws_cloudhsm(
        cu_user: impl Into<String>,
        key_label: impl Into<String>,
        cu_password: impl Into<String>,
    ) -> Self {
        let prof = profile(HsmProvider::Aws);
        Self {
            provider: HsmProvider::Aws,
            library_path: PathBuf::from(prof.default_library),
            slot: None,
            token_label: None,
            key_label: key_label.into(),
            key_id: None,
            user_pin: format!("{}:{}", cu_user.into(), cu_password.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aws_env_config_builds_cloudhsm_pin_and_defaults() {
        let cfg = HsmConfig::from_env_aws_with("crypto_user", "validator-key", |_| {
            Ok("secret".to_string())
        });
        assert!(cfg.is_ok(), "aws hsm config failed: {cfg:?}");
        let Ok(cfg) = cfg else {
            return;
        };

        assert_eq!(cfg.provider, HsmProvider::Aws);
        assert_eq!(
            cfg.library_path,
            PathBuf::from(profile(HsmProvider::Aws).default_library)
        );
        assert_eq!(cfg.slot, None);
        assert_eq!(cfg.token_label, None);
        assert_eq!(cfg.key_label, "validator-key");
        assert_eq!(cfg.key_id, None);
        assert_eq!(cfg.user_pin, "crypto_user:secret");
    }

    #[test]
    fn aws_env_config_returns_init_error_when_pin_env_is_missing() {
        let err = HsmConfig::from_env_aws_with("crypto_user", "validator-key", |_| {
            Err(std::env::VarError::NotPresent)
        })
        .expect_err("missing pin env must be an init error");

        assert!(matches!(err, HsmError::Init(message) if message.contains("NEO_HSM_CU_PASSWORD")));
    }
}
