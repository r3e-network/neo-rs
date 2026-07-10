//! Native Azure Key Vault / Managed HSM signer (feature `azure`).
//!
//! This backend is **for Azure Managed HSM and Key Vault Premium**, whose
//! PKCS#11 library (`mhsm-pkcs11`) is restricted by Microsoft to TLS/SSL
//! offload with F5/Nginx only and is not usable as a general signer.  The
//! general-purpose signing interface is a REST call:
//!
//! ```text
//! POST https://{vault}.{managedhsm|vault}.azure.net/keys/{name}/{version}/sign?api-version=7.4
//! Body: {"alg":"ES256","value":"<base64url SHA-256 digest>"}
//! Response: {"value":"<base64url raw r||s 64 bytes>"}
//! ```
//!
//! ES256 (RFC 7518 §3.4) returns the signature as raw `r‖s` base64url —
//! directly Neo-compatible after low-s normalization.
//!
//! # Feature status
//!
//! The `azure` feature compiles this module.  The REST wiring is **implemented**
//! using `reqwest` blocking for synchronous `ConsensusSigner::sign` compliance
//! and `base64` for base64url encode/decode.  Azure Entra ID token acquisition
//! uses a hardcoded bearer-token env variable (`AZURE_BEARER_TOKEN`) because
//! pulling in the full `azure_identity` SDK is out of scope; operators in
//! production should supply a short-lived token from their identity provider.
//!
//! If a full `azure_identity` integration is required, replace the
//! `AZURE_BEARER_TOKEN` path with the official `azure_security_keyvault_keys`
//! crate (GA'd 2026-05).

use crate::error::{HsmError, HsmResult};
use neo_consensus::ConsensusSigner;
use neo_consensus::error::ConsensusError;
use neo_crypto::{Crypto, Secp256r1Crypto};
use neo_primitives::UInt160;

/// Configuration for the Azure Key Vault / Managed HSM native REST signer.
#[derive(Debug, Clone)]
pub struct AzureKeyVaultConfig {
    /// Base URL of the vault.
    ///
    /// For Managed HSM: `https://<name>.managedhsm.azure.net`
    /// For Key Vault Premium: `https://<name>.vault.azure.net`
    pub vault_url: String,

    /// Name of the key in Key Vault.
    pub key_name: String,

    /// Version of the key (empty string = latest).
    pub key_version: String,

    /// Azure Neo script hash (must be provided externally, e.g. from operator
    /// registration of the pubkey).  The REST signer does not derive this
    /// from the HSM — the operator must supply the correct UInt160.
    pub script_hash: UInt160,

    /// Azure API version to use.  Defaults to `"7.4"`.
    pub api_version: String,
}

impl Default for AzureKeyVaultConfig {
    fn default() -> Self {
        Self {
            vault_url: String::new(),
            key_name: String::new(),
            key_version: String::new(),
            script_hash: UInt160::zero(),
            api_version: "7.4".to_string(),
        }
    }
}

/// Azure Key Vault / Managed HSM native REST signer.
///
/// Uses async `reqwest::Client` for non-blocking `ConsensusSigner::sign`.
/// Bearer token is sourced from the `AZURE_BEARER_TOKEN` environment variable.
pub struct AzureKeyVaultSigner {
    cfg: AzureKeyVaultConfig,
    client: reqwest::Client,
}

impl AzureKeyVaultSigner {
    /// Create a new signer from the supplied configuration.
    ///
    /// Reads `AZURE_BEARER_TOKEN` from the environment at call time.
    /// Validates that the vault URL and key name are non-empty.
    pub fn new(cfg: AzureKeyVaultConfig) -> HsmResult<Self> {
        if cfg.vault_url.is_empty() {
            return Err(HsmError::Init("azure: vault_url must not be empty".into()));
        }
        if cfg.key_name.is_empty() {
            return Err(HsmError::Init("azure: key_name must not be empty".into()));
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| HsmError::Init(format!("azure: reqwest client: {e}")))?;
        Ok(Self { cfg, client })
    }

    /// Perform the async REST sign call and return the 64-byte low-s `r‖s`.
    async fn rest_sign(&self, data: &[u8]) -> HsmResult<Vec<u8>> {
        let bearer = std::env::var("AZURE_BEARER_TOKEN")
            .map_err(|_| HsmError::Init("azure: AZURE_BEARER_TOKEN env var not set".into()))?;

        // Compute SHA-256 digest locally (Key Vault signs a *hash*, not raw data).
        let digest = Crypto::sha256(data);

        // base64url-encode the 32-byte digest (no padding, URL-safe alphabet).
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        let value_b64 = URL_SAFE_NO_PAD.encode(digest);

        // Build the REST URL.
        let version_segment = if self.cfg.key_version.is_empty() {
            String::new()
        } else {
            format!("/{}", self.cfg.key_version)
        };
        let url = format!(
            "{}/keys/{}{}/sign?api-version={}",
            self.cfg.vault_url.trim_end_matches('/'),
            self.cfg.key_name,
            version_segment,
            self.cfg.api_version,
        );

        // POST {"alg":"ES256","value":"<base64url>"}
        let body = serde_json::json!({
            "alg": "ES256",
            "value": value_b64,
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&bearer)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(HsmError::AzureHttp)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(HsmError::Sign(format!("azure: HTTP {status}: {text}")));
        }

        // Parse {"value":"<base64url raw r||s>"}
        let json: serde_json::Value = resp.json().await.map_err(HsmError::AzureHttp)?;
        let sig_b64 = json["value"]
            .as_str()
            .ok_or_else(|| HsmError::Sign("azure: missing 'value' in response".into()))?;

        let raw = URL_SAFE_NO_PAD
            .decode(sig_b64)
            .map_err(|e| HsmError::SigDecode(format!("azure: base64url decode: {e}")))?;

        if raw.len() != 64 {
            return Err(HsmError::UnexpectedSigLen {
                expected: 64,
                got: raw.len(),
            });
        }

        // Low-s normalize (Azure ES256 may return high-s on P-256).
        let mut buf = [0u8; 64];
        buf.copy_from_slice(&raw);
        let normalized = Secp256r1Crypto::normalize_low_s(&buf)
            .map_err(|e| HsmError::Normalize(format!("{e}")))?;
        Ok(normalized.to_vec())
    }
}

impl ConsensusSigner for AzureKeyVaultSigner {
    fn can_sign(&self, script_hash: &UInt160) -> bool {
        *script_hash == self.cfg.script_hash
    }

    async fn sign(&self, data: &[u8], script_hash: &UInt160) -> Result<Vec<u8>, ConsensusError> {
        if !self.can_sign(script_hash) {
            return Err(ConsensusError::state_error(format!(
                "hsm-azure: unknown script hash {script_hash}"
            )));
        }
        self.rest_sign(data).await.map_err(ConsensusError::from)
    }
}
