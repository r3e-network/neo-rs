//! Advisory preverification for canonical Neo signature witnesses.
//!
//! ## Boundary
//!
//! This module owns a bounded, immutable set of exact ECDSA verification
//! outcomes produced from canonical standard witness scripts. It does not
//! authorize a witness, skip NeoVM execution, or accept externally supplied
//! cache entries.
//!
//! ## Contents
//!
//! - [`PreverifiedSignatureCache`]: opaque exact-key outcomes for one Neo sign
//!   data value.
//! - [`PreverifiedSignatureCacheMetricsSnapshot`]: bounded per-cache canonical
//!   consumption and lookup counters.
//! - [`preverify_standard_witness_signatures`]: pure standard-witness
//!   preverification with deterministic fallback for unsupported shapes.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use neo_crypto::{CryptoError, Secp256r1Crypto};
use neo_payloads::Witness;
use neo_vm::script_builder::redeem_script::RedeemScript;
use rustc_hash::FxHashMap;

const NEO_SIGN_DATA_LEN: usize = 36;
const COMPRESSED_PUBLIC_KEY_LEN: usize = 33;
const RAW_SIGNATURE_LEN: usize = 64;
const CACHE_KEY_LEN: usize = COMPRESSED_PUBLIC_KEY_LEN + RAW_SIGNATURE_LEN;
const MAX_PREVERIFIED_SIGNATURES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct SignatureCacheKey([u8; CACHE_KEY_LEN]);

impl SignatureCacheKey {
    fn new(public_key: &[u8], signature: &[u8]) -> Option<Self> {
        if public_key.len() != COMPRESSED_PUBLIC_KEY_LEN || signature.len() != RAW_SIGNATURE_LEN {
            return None;
        }

        let mut bytes = [0u8; CACHE_KEY_LEN];
        bytes[..COMPRESSED_PUBLIC_KEY_LEN].copy_from_slice(public_key);
        bytes[COMPRESSED_PUBLIC_KEY_LEN..].copy_from_slice(signature);
        Some(Self(bytes))
    }
}

/// Immutable advisory outcomes for exact signature-verification inputs.
///
/// Instances can only be produced by [`preverify_standard_witness_signatures`].
/// Each instance is bound to one exact Neo `network || hash` sign-data value
/// and retains at most 64 exact compressed-public-key/signature pairs. A miss
/// always selects ordinary secp256r1 verification.
#[derive(Debug)]
pub struct PreverifiedSignatureCache {
    sign_data: [u8; NEO_SIGN_DATA_LEN],
    entries: FxHashMap<SignatureCacheKey, bool>,
    canonical_uses: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
}

/// Point-in-time observability for one immutable preverified-signature cache.
///
/// Counters are local to one bounded cache and carry no input labels. They do
/// not alter exact-key outcomes or authorize a witness.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PreverifiedSignatureCacheMetricsSnapshot {
    /// Times the cache was installed on a canonical application engine.
    pub canonical_uses: u64,
    /// Exact cache lookups attempted by canonical signature verification.
    pub lookups: u64,
    /// Lookups that reused a preverified boolean outcome.
    pub hits: u64,
    /// Lookups that fell back to ordinary secp256r1 verification.
    pub misses: u64,
}

impl PreverifiedSignatureCache {
    fn new(sign_data: &[u8]) -> Option<Self> {
        Some(Self {
            sign_data: sign_data.try_into().ok()?,
            entries: FxHashMap::default(),
            canonical_uses: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        })
    }

    fn insert(&mut self, public_key: &[u8], signature: &[u8], outcome: bool) -> bool {
        if self.entries.len() >= MAX_PREVERIFIED_SIGNATURES {
            return false;
        }
        let Some(key) = SignatureCacheKey::new(public_key, signature) else {
            return false;
        };
        self.entries.insert(key, outcome);
        true
    }

    /// Returns the number of exact ECDSA operations retained by this cache.
    ///
    /// This exposes bounded aggregate observability without exposing cache
    /// keys, outcomes, or mutation. A canonical NeoVM cache miss still falls
    /// back to ordinary signature verification.
    #[must_use]
    pub fn operation_count(&self) -> usize {
        self.entries.len()
    }

    /// Returns a point-in-time snapshot of canonical cache activity.
    #[must_use]
    pub fn metrics_snapshot(&self) -> PreverifiedSignatureCacheMetricsSnapshot {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        PreverifiedSignatureCacheMetricsSnapshot {
            canonical_uses: self.canonical_uses.load(Ordering::Relaxed),
            lookups: hits.saturating_add(misses),
            hits,
            misses,
        }
    }

    pub(crate) fn record_canonical_use(&self) {
        self.canonical_uses.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn lookup(
        &self,
        sign_data: &[u8],
        public_key: &[u8],
        signature: &[u8],
    ) -> Option<bool> {
        let outcome = (sign_data == self.sign_data)
            .then(|| SignatureCacheKey::new(public_key, signature))
            .flatten()
            .and_then(|key| self.entries.get(&key).copied());
        match outcome {
            Some(outcome) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(outcome)
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }
}

/// Preverifies the ECDSA operations reachable from one canonical standard witness.
///
/// The function accepts only Neo's 36-byte sign data and canonical single- or
/// multi-signature invocation and verification scripts. Unsupported or
/// malformed shapes return `None`. A large multisig may return a partial cache
/// after the fixed 64-operation bound; uncached operations remain ordinary VM
/// crypto calls.
#[must_use]
pub fn preverify_standard_witness_signatures(
    sign_data: &[u8],
    witness: &Witness,
) -> Option<Arc<PreverifiedSignatureCache>> {
    let mut cache = PreverifiedSignatureCache::new(sign_data)?;

    if RedeemScript::is_signature_contract(&witness.verification_script) {
        preverify_single_signature(sign_data, witness, &mut cache)?;
        return Some(Arc::new(cache));
    }

    preverify_multi_signature(sign_data, witness, &mut cache)?;
    Some(Arc::new(cache))
}

fn preverify_single_signature(
    sign_data: &[u8],
    witness: &Witness,
    cache: &mut PreverifiedSignatureCache,
) -> Option<()> {
    let signatures = RedeemScript::parse_multi_sig_invocation(&witness.invocation_script, 1)?;
    let public_key = witness.verification_script.get(2..35)?;
    let signature = signatures.first()?;
    let outcome = verify_boolean_outcome(sign_data, public_key, signature)?;
    cache.insert(public_key, signature, outcome).then_some(())
}

fn preverify_multi_signature(
    sign_data: &[u8],
    witness: &Witness,
    cache: &mut PreverifiedSignatureCache,
) -> Option<()> {
    let (required, mut public_keys) =
        RedeemScript::parse_multi_sig_contract(&witness.verification_script)?;
    let mut signatures =
        RedeemScript::parse_multi_sig_invocation(&witness.invocation_script, required)?;

    // Standard scripts push both collections in forward bytecode order. NeoVM
    // then pops each count followed by its elements, producing reverse-order
    // vectors for CheckMultisig. Mirror that exact traversal so false scan
    // pairs, not only successful matches, are available to the canonical VM.
    public_keys.reverse();
    signatures.reverse();

    let mut verified = 0usize;
    let mut key_index = 0usize;
    let mut attempts = 0usize;
    for signature in &signatures {
        while key_index < public_keys.len() {
            if attempts == MAX_PREVERIFIED_SIGNATURES {
                return Some(());
            }
            attempts += 1;

            let public_key = &public_keys[key_index];
            let outcome = match verify_boolean_outcome(sign_data, public_key, signature) {
                Some(outcome) => outcome,
                None => return (!cache.entries.is_empty()).then_some(()),
            };
            if !cache.insert(public_key, signature, outcome) {
                return Some(());
            }
            key_index += 1;

            if outcome {
                verified += 1;
                break;
            }
        }

        if required - verified > public_keys.len() - key_index {
            break;
        }
    }

    (!cache.entries.is_empty()).then_some(())
}

fn verify_boolean_outcome(sign_data: &[u8], public_key: &[u8], signature: &[u8]) -> Option<bool> {
    let signature = <&[u8; RAW_SIGNATURE_LEN]>::try_from(signature).ok()?;
    match Secp256r1Crypto::verify(sign_data, signature, public_key) {
        Ok(outcome) => Some(outcome),
        Err(CryptoError::InvalidSignature { .. }) => Some(false),
        Err(_) => None,
    }
}

#[cfg(test)]
#[path = "../tests/verification/preverified_signatures.rs"]
mod tests;
