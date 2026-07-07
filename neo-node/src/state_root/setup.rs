//! StateService validator setup and key parsing.
//!
//! This module resolves whether the node participates as a StateValidator or as
//! an observer. It does not own voting, payload routing, or persistence.

use neo_config::ProtocolSettings;
use neo_crypto::{ECPoint, Secp256r1Crypto};
use neo_primitives::hex_util;

/// This node's optional StateValidator identity and the timing it needs.
pub struct StateRootSetup {
    /// The StateValidator signing key + its public point, or `None` when this
    /// node only verifies and persists inbound signed roots (observer).
    pub keypair: Option<([u8; 32], ECPoint)>,
    /// Network magic (signed into vote and extensible sign-data).
    pub network: u32,
    /// Target block time (ms) — the retry-backoff base.
    pub ms_per_block: u64,
}

/// Resolves the StateService driver setup. Returns `Ok(None)` when the state
/// service is disabled (no local roots to attest). When a validator key is
/// configured it is parsed into a keypair; otherwise the node runs as an
/// observer that still verifies and persists inbound signed roots.
pub fn build_state_root_setup(
    settings: &ProtocolSettings,
    state_service_enabled: bool,
    validator_key_hex: Option<&str>,
) -> anyhow::Result<Option<StateRootSetup>> {
    if !state_service_enabled {
        return Ok(None);
    }
    let keypair = match validator_key_hex {
        Some(hex_key) => {
            let raw = hex_util::decode_hex(hex_key.trim())
                .map_err(|e| anyhow::anyhow!("invalid state validator private key hex: {e}"))?;
            let private_key: [u8; 32] = raw
                .as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("state validator private key must be 32 bytes"))?;
            let public_key = ECPoint::from_bytes(
                &Secp256r1Crypto::derive_public_key(&private_key)
                    .map_err(|e| anyhow::anyhow!("failed to derive state validator key: {e}"))?,
            )
            .map_err(|e| anyhow::anyhow!("failed to decode state validator key: {e}"))?;
            Some((private_key, public_key))
        }
        None => None,
    };
    Ok(Some(StateRootSetup {
        keypair,
        network: settings.network,
        ms_per_block: u64::from(settings.milliseconds_per_block),
    }))
}
