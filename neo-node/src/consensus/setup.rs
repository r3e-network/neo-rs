//! Consensus validator-set and signer setup.
//!
//! This module resolves the ordered dBFT validator set, this node's validator
//! index, and software/HSM signing configuration. The async consensus driver
//! consumes the resulting [`ConsensusSetup`] but does not own config parsing.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_consensus::{ConsensusSigner, ValidatorInfo};
use neo_crypto::{ECPoint, Secp256r1Crypto};
use neo_primitives::{UInt160, hex_util};
use neo_vm::script_builder::RedeemScript;
use tracing::warn;

use super::hsm::{HsmKeyConfig, build_hsm_consensus_setup};

pub(super) fn validator_infos_from_keys(keys: Vec<ECPoint>) -> Vec<ValidatorInfo> {
    keys.into_iter()
        .enumerate()
        .map(|(index, public_key)| {
            let script_hash = UInt160::from_script(&RedeemScript::signature_redeem_script(
                public_key.as_bytes(),
            ));
            ValidatorInfo {
                index: index as u8,
                public_key,
                script_hash,
            }
        })
        .collect()
}

/// Builds the ordered dBFT validator set from the protocol settings.
///
/// C# dBFT uses `NEO.GetNextBlockValidators(...).OrderBy(p => p)`, which at
/// genesis reduces to `StandbyCommittee.Take(ValidatorsCount).OrderBy(p => p)`.
/// `standby_validators()` does the `Take(N)` but NOT the sort; the validator
/// **index** (and thus primary selection) depends on the sorted order, so the
/// keys are sorted here.
pub fn build_consensus_validators(settings: &ProtocolSettings) -> Vec<ValidatorInfo> {
    let mut keys: Vec<ECPoint> = settings.standby_validators();
    keys.sort();
    validator_infos_from_keys(keys)
}

/// Finds this node's validator index by deriving its public key from the
/// private key and matching it against the (sorted) validator set. `None` when
/// this node is not a consensus validator (it then only relays).
pub fn resolve_my_index(private_key: &[u8; 32], validators: &[ValidatorInfo]) -> Option<u8> {
    let pub_bytes = Secp256r1Crypto::derive_public_key(private_key).ok()?;
    let my_pubkey = ECPoint::from_bytes(&pub_bytes).ok()?;
    resolve_public_key_index(&my_pubkey, validators)
}

pub(super) fn resolve_public_key_index(
    public_key: &ECPoint,
    validators: &[ValidatorInfo],
) -> Option<u8> {
    validators
        .iter()
        .find(|v| &v.public_key == public_key)
        .map(|v| v.index)
}

/// Resolved consensus configuration: the validator set and this node's key/index.
pub struct ConsensusSetup {
    /// The ordered dBFT validator set.
    pub validators: Vec<ValidatorInfo>,
    /// This node's public key, derived from `private_key`.
    pub public_key: ECPoint,
    /// This node's validator index, or `None` (observer; relay-only).
    pub my_index: Option<u8>,
    /// This node's 32-byte secp256r1 software private key. Zeroed and unused
    /// when `signer` is set (HSM-backed signing).
    pub private_key: [u8; 32],
    /// Network magic.
    pub network: u32,
    /// Target block time (ms) - the view-timeout base.
    pub ms_per_block: u64,
    /// Optional HSM-backed signer. When `Some`, the consensus service signs via
    /// this signer (keyed by the validator script hash) instead of `private_key`.
    pub signer: Option<Arc<dyn ConsensusSigner>>,
}

/// Builds the consensus setup from the protocol settings and the `[consensus]`
/// configuration. Returns `Ok(None)` when consensus is disabled. Returns an
/// error when consensus is enabled but the validator key is missing/malformed.
pub fn build_consensus_setup(
    settings: &ProtocolSettings,
    enabled: bool,
    private_key_hex: Option<&str>,
    hsm: Option<&HsmKeyConfig>,
) -> anyhow::Result<Option<ConsensusSetup>> {
    if !enabled {
        return Ok(None);
    }

    let validators = build_consensus_validators(settings);

    // HSM-backed signing takes precedence over a software key when configured.
    if let Some(hsm_cfg) = hsm {
        if private_key_hex.is_some() {
            warn!("[consensus] both private_key_hex and [consensus.hsm] are set; using the HSM");
        }
        return build_hsm_consensus_setup(settings, validators, hsm_cfg);
    }

    let hex_key = private_key_hex.ok_or_else(|| {
        anyhow::anyhow!(
            "[consensus].enabled = true requires [consensus].private_key_hex or [consensus.hsm]"
        )
    })?;
    let raw = hex_util::decode_hex(hex_key.trim())
        .map_err(|e| anyhow::anyhow!("invalid [consensus].private_key_hex: {e}"))?;
    let private_key: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("[consensus].private_key_hex must be 32 bytes"))?;
    let public_key = ECPoint::from_bytes(
        &Secp256r1Crypto::derive_public_key(&private_key)
            .map_err(|e| anyhow::anyhow!("failed to derive consensus public key: {e}"))?,
    )
    .map_err(|e| anyhow::anyhow!("failed to decode consensus public key: {e}"))?;

    let my_index = resolve_my_index(&private_key, &validators);
    Ok(Some(ConsensusSetup {
        validators,
        public_key,
        my_index,
        private_key,
        network: settings.network,
        ms_per_block: u64::from(settings.milliseconds_per_block),
        signer: None,
    }))
}
