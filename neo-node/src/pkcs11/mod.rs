//! PKCS#11 HSM adapter for the `hardware_tee` signer provider.
//!
//! The module separates three concerns:
//!
//! - [`Pkcs11Backend`]: transport-agnostic trait abstracting slot/token
//!   discovery, mechanism negotiation, public-key export, and authorized
//!   signing against a PKCS#11 token.
//! - [`LibPkcs11Backend`]: thin hand-written C FFI layer over a dynamically
//!   loaded PKCS#11 v2.40 library (`C_Initialize`, `C_OpenSession`,
//!   `C_Login`, `C_Sign`, ...). Loading fails closed.
//! - [`HsmSigner`]: `SignerProvider` implementation that validates every
//!   `KeyRef`, enforces purpose authorization, and binds the configured
//!   account public key to the key exported by the HSM.
//!
//! Tests inject [`mock::MockP11Backend`], a pure-Rust in-memory token; real
//! HSM hardware remains an external acceptance item.

mod backend;
pub(crate) mod lib_backend;
#[cfg(test)]
pub(crate) mod mock;
pub(crate) mod provider;
#[cfg(test)]
pub(crate) mod test_env;

pub(crate) use backend::Pkcs11Backend;
#[cfg(test)]
pub(crate) use backend::Pkcs11Mechanism;
pub(crate) use lib_backend::LibPkcs11Backend;
pub(crate) use provider::HsmSigner;

use anyhow::{Result, bail};
use zeroize::Zeroizing;

/// Environment variable carrying the HSM user PIN.
///
/// The PIN is deliberately never accepted from TOML configuration so no
/// secret material is persisted in node configuration files.
pub(crate) const NEO_HSM_PIN_ENV: &str = "NEO_HSM_PIN";

/// Reads and trims the HSM user PIN from the environment, failing closed when
/// the variable is missing or blank.
pub(crate) fn hsm_pin_from_env() -> Result<Zeroizing<Vec<u8>>> {
    let raw = std::env::var(NEO_HSM_PIN_ENV).map_err(|_| {
        anyhow::anyhow!(
            "HSM user PIN environment variable '{NEO_HSM_PIN_ENV}' is not set; \
             refusing to construct the hardware TEE signer provider without an explicit PIN"
        )
    })?;
    let pin = Zeroizing::new(raw.trim().as_bytes().to_vec());
    if pin.as_slice().is_empty() {
        bail!("HSM user PIN environment variable '{NEO_HSM_PIN_ENV}' must not be blank");
    }
    Ok(pin)
}
