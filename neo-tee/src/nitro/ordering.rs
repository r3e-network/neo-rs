//! Verifiable fair-ordering proofs produced inside the enclave.
//!
//! The enclave runs the existing fair-ordering sequencer (`crate::mempool`)
//! over a batch of transactions, then wraps the result in an
//! [`OrderingProof`]: the ordered tx-hash list, the sequencer's own signed
//! merkle-root proof ([`crate::mempool::tee_mempool::OrderingProof`]), and a
//! Nitro attestation document binding a digest of that proof. Peers receiving a
//! proposed block can call [`verify_ordering_proof`] to check the proof is
//! internally consistent and (in production) that the attestation is genuine.
//!
//! Reference: `claudedocs/aws-hsm-nitro-tee-design.md` §3.2, §4.3.
//!
//! # What is real vs experimental here
//!
//! * Producing the ordered list and the merkle-root signature is REAL — it
//!   reuses [`crate::mempool::TeeMempool`] verbatim and is tested.
//! * Binding the proof to an attestation document is REAL in shape (the
//!   `attestation` bytes carry whatever the platform's `attest` returns).
//! * Verifying that attestation against the Nitro PKI is EXPERIMENTAL — it
//!   defers to [`crate::nitro::attestation::verify_pki_chain`], which is a stub.
//!   [`verify_ordering_proof`] therefore reports the attestation trust status
//!   separately so a caller cannot mistake "structurally valid" for "attested".

use crate::error::{TeeError, TeeResult};
use crate::mempool::TeeMempool;
use crate::mempool::tee_mempool::OrderingProof as SequencerProof;
use crate::nitro::vsock::OrderTxEntry;
use neo_crypto::Secp256r1Crypto;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha384};
use std::sync::Arc;

/// A verifiable fair-ordering proof returned by the enclave.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderingProof {
    /// Transaction hashes in the fair order the enclave selected.
    pub ordered_hashes: Vec<[u8; 32]>,
    /// The sequencer's signed proof over the merkle root of `ordered_hashes`.
    pub sequencer_proof: SequencerProof,
    /// Serialized Nitro attestation document binding a digest of the proof.
    ///
    /// Empty when the platform produced no document (e.g. the simulation
    /// platform with attestation disabled); a real Nitro platform always
    /// populates it.
    pub attestation: Vec<u8>,
}

impl OrderingProof {
    /// Computes the user-data digest that the attestation document binds.
    ///
    /// `SHA384(merkle_root || enclave_counter_le || policy_hash)` — matches the
    /// design's `user_data = SHA384(merkle_root || enclave_counter || policy_hash)`.
    #[must_use]
    pub fn attestation_user_data(proof: &SequencerProof) -> [u8; 48] {
        let mut hasher = Sha384::new();
        hasher.update(proof.merkle_root);
        hasher.update(proof.enclave_counter.to_le_bytes());
        hasher.update(proof.policy_hash);
        let digest = hasher.finalize();
        let mut out = [0u8; 48];
        out.copy_from_slice(&digest);
        out
    }
}

/// Builds an [`OrderingProof`] by running the sequencer over `txs`.
///
/// Inserts each transaction into `sequencer`, generates the sequencer's signed
/// ordering proof, reads back the ordered hashes (capped at `limit`), and
/// attaches `attestation` (the bytes a platform `attest` returned over
/// [`OrderingProof::attestation_user_data`]). Transactions already present in
/// the sequencer are skipped rather than treated as errors, so re-submitting a
/// batch is idempotent.
///
/// # Errors
///
/// Returns a [`TeeError`] if the sequencer rejects an insertion for a reason
/// other than duplication (e.g. capacity) or if proof generation fails.
pub fn build_ordering_proof(
    sequencer: &Arc<TeeMempool>,
    txs: &[OrderTxEntry],
    limit: usize,
    attestation: Vec<u8>,
) -> TeeResult<OrderingProof> {
    for tx in txs {
        if sequencer.contains(&tx.tx_hash) {
            continue;
        }
        match sequencer.add_transaction(
            tx.tx_hash,
            // The ordering decision does not need the tx body; an empty payload
            // keeps the proof small. The hash is the identity that matters.
            Vec::new(),
            tx.network_fee,
            tx.system_fee,
            tx.sender,
        ) {
            Ok(_) => {}
            Err(TeeError::Other(msg)) if msg.contains("already in pool") => {}
            Err(e) => return Err(e),
        }
    }

    let sequencer_proof = sequencer.generate_ordering_proof()?;
    let ordered_hashes = sequencer.get_ordered_hashes(limit);

    Ok(OrderingProof {
        ordered_hashes,
        sequencer_proof,
        attestation,
    })
}

/// Outcome of verifying an [`OrderingProof`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderingVerification {
    /// The merkle root over `ordered_hashes` matches `sequencer_proof.merkle_root`.
    pub merkle_root_matches: bool,
    /// The sequencer's secp256r1 signature over the proof tuple verifies under
    /// the embedded public key.
    pub signature_valid: bool,
    /// Whether the attached attestation document was cryptographically verified
    /// against the Nitro PKI.
    ///
    /// Currently always `false` because PKI verification is an experimental
    /// stub — a caller in production MUST treat `false` as "do not trust the
    /// origin enclave" unless it has independently established the pubkey.
    pub attestation_verified: bool,
}

impl OrderingVerification {
    /// Returns `true` only when the proof is internally consistent AND the
    /// attestation was verified. This is the strict, production-grade check.
    #[must_use]
    pub fn is_fully_trusted(&self) -> bool {
        self.merkle_root_matches && self.signature_valid && self.attestation_verified
    }

    /// Returns `true` when the proof is internally consistent (merkle root +
    /// signature), independent of attestation trust. Useful for the advisory
    /// (opt-in) v1 path where peers check consistency but defer attestation.
    #[must_use]
    pub fn is_internally_consistent(&self) -> bool {
        self.merkle_root_matches && self.signature_valid
    }
}

/// Verifies an [`OrderingProof`] a peer received.
///
/// Checks, independently:
/// 1. that the merkle root recomputed over `ordered_hashes` equals the merkle
///    root the sequencer signed;
/// 2. that the sequencer's signature over
///    `merkle_root || counter_le || policy_hash` verifies under the embedded
///    public key;
/// 3. (EXPERIMENTAL) that the attestation document is genuine — deferred to the
///    PKI stub and reported via `attestation_verified`.
///
/// Returns the breakdown so the caller decides its trust policy
/// ([`OrderingVerification::is_fully_trusted`] vs `is_internally_consistent`).
#[must_use]
pub fn verify_ordering_proof(proof: &OrderingProof) -> OrderingVerification {
    let merkle_root_matches =
        merkle_root(&proof.ordered_hashes) == proof.sequencer_proof.merkle_root;

    let signature_valid = verify_sequencer_signature(&proof.sequencer_proof);

    let attestation_verified = verify_attestation(proof);

    OrderingVerification {
        merkle_root_matches,
        signature_valid,
        attestation_verified,
    }
}

/// Recomputes the merkle root over an ordered hash list using the same scheme
/// the sequencer uses (`SHA-256(left || right)`, duplicating the last leaf on
/// an odd level). This MUST stay byte-identical to
/// `TeeMempool::compute_merkle_root`.
fn merkle_root(hashes: &[[u8; 32]]) -> [u8; 32] {
    if hashes.is_empty() {
        return [0u8; 32];
    }
    let mut level: Vec<[u8; 32]> = hashes.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for chunk in level.chunks(2) {
            let right = if chunk.len() > 1 {
                &chunk[1]
            } else {
                &chunk[0]
            };
            let mut data = [0u8; 64];
            data[..32].copy_from_slice(&chunk[0]);
            data[32..].copy_from_slice(right);
            next.push(neo_crypto::Crypto::sha256(&data));
        }
        level = next;
    }
    level[0]
}

fn verify_sequencer_signature(proof: &SequencerProof) -> bool {
    if proof.signature.len() != 64 {
        return false;
    }
    let mut message = Vec::with_capacity(32 + 8 + 32);
    message.extend_from_slice(&proof.merkle_root);
    message.extend_from_slice(&proof.enclave_counter.to_le_bytes());
    message.extend_from_slice(&proof.policy_hash);

    let sig: [u8; 64] = match proof.signature.as_slice().try_into() {
        Ok(s) => s,
        Err(_) => return false,
    };
    Secp256r1Crypto::verify(&message, &sig, &proof.public_key).unwrap_or(false)
}

/// Verifies the attestation document bound to an ordering proof.
///
/// # EXPERIMENTAL: validate in a Nitro environment before production
///
/// Parses the document and confirms its `user_data` binds the proof, but the
/// PKI/COSE trust decision defers to the stubbed
/// [`crate::nitro::attestation::verify_pki_chain`]. Until that is implemented
/// against the pinned Root G1, this returns `false` (untrusted) for any real
/// document.
fn verify_attestation(proof: &OrderingProof) -> bool {
    if proof.attestation.is_empty() {
        return false;
    }
    let Ok(envelope) = crate::nitro::attestation::parse_cose_sign1(&proof.attestation) else {
        return false;
    };
    let Ok(doc) = crate::nitro::attestation::NitroAttestationDoc::parse_payload(&envelope.payload)
    else {
        return false;
    };

    // The bound user_data must equal the digest of the proof being attested.
    let expected = OrderingProof::attestation_user_data(&proof.sequencer_proof);
    if doc.user_data.as_deref() != Some(expected.as_slice()) {
        return false;
    }

    // EXPERIMENTAL: PKI/COSE verification is a stub; never report Verified yet.
    matches!(
        crate::nitro::attestation::verify_pki_chain(
            &doc,
            &envelope,
            &crate::nitro::attestation::NITRO_ROOT_G1_SHA256_FINGERPRINT,
        ),
        crate::nitro::attestation::PkiVerification::Verified
    )
}

#[cfg(test)]
#[path = "../tests/nitro/ordering.rs"]
mod tests;
