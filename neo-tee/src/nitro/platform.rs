//! TEE platform abstraction shared by Nitro (and future SGX) backends.
//!
//! `TeePlatform` is the seam that isolates *where* attestation, sealing, and
//! fair-ordering execution physically happen from the protocol-level logic that
//! consumes them. The Nitro backend implements this trait against the AWS NSM
//! driver and the in-enclave sequencer; an SGX backend (future work) would
//! implement the same trait against `EGETKEY` / DCAP, and a `Simulation`
//! implementation provides a hardware-free path for tests.
//!
//! Design reference: `claudedocs/aws-hsm-nitro-tee-design.md` §3.2.
//!
//! ## What the seam abstracts
//!
//! 1. `attest(user_data, nonce, public_key)` — produce a platform attestation
//!    document binding `user_data` (and optionally a public key) to the
//!    measured enclave image. On Nitro this is a COSE_Sign1 NSM document; on SGX
//!    it would be a DCAP quote.
//! 2. `seal` / `unseal` — protect a 32-byte key at rest using a platform-rooted
//!    sealing key. On Nitro there is no `EGETKEY`, so a real implementation must
//!    root sealing in a KMS-wrapped blob (see the design doc §5.2); on SGX it is
//!    `EGETKEY`-derived.
//! 3. `sequencer()` — access to the fair-ordering sequencer that runs inside the
//!    trusted boundary, reused verbatim from `crate::mempool`.
//!
//! Keeping these three behind one trait means a Nitro validator and an SGX
//! validator are interchangeable from the perspective of the host-side signer
//! and the ordering-proof verifier.

use crate::error::TeeResult;
use crate::mempool::TeeMempool;
use std::sync::Arc;

/// Identifies which concrete TEE implementation backs a [`TeePlatform`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformKind {
    /// Intel SGX (DCAP / EGETKEY). Future work — not implemented in this module.
    Sgx,
    /// AWS Nitro Enclaves (NSM attestation + KMS-rooted sealing).
    Nitro,
    /// Hardware-free simulation used by tests and local development.
    Simulation,
}

impl PlatformKind {
    /// Returns `true` if this platform provides a genuine hardware root of trust.
    ///
    /// [`PlatformKind::Simulation`] returns `false`; its attestation documents
    /// are self-signed and MUST NOT be trusted as evidence of enclave execution.
    #[must_use]
    pub fn is_hardware(&self) -> bool {
        matches!(self, PlatformKind::Sgx | PlatformKind::Nitro)
    }
}

/// Abstracts the trusted-execution platform behind attestation, sealing, and
/// the fair-ordering sequencer.
///
/// Implementors run inside (or directly in front of) the trusted boundary. The
/// trait is `Send + Sync` so a platform handle can be shared across the enclave
/// server's accept loop.
pub trait TeePlatform: Send + Sync {
    /// Returns which concrete platform backs this implementation.
    fn kind(&self) -> PlatformKind;

    /// Produces a platform attestation document.
    ///
    /// * `user_data` — application bytes to bind into the document (e.g. a hash
    ///   of an ordering proof). May be empty.
    /// * `nonce` — caller-supplied freshness/replay-protection bytes. May be
    ///   empty for documents that are not challenge-response.
    /// * `public_key` — an optional public key to bind into the document (used
    ///   by the KMS attested-decrypt import path to carry the ephemeral RSA key).
    ///
    /// Returns the serialized document bytes (a COSE_Sign1 envelope on Nitro).
    fn attest(
        &self,
        user_data: &[u8],
        nonce: &[u8],
        public_key: Option<&[u8]>,
    ) -> TeeResult<Vec<u8>>;

    /// Seals a 32-byte key with the platform-rooted sealing key.
    ///
    /// Returns an opaque sealed blob that only this platform (same measured
    /// image / same sealing root) can [`TeePlatform::unseal`].
    fn seal(&self, key: &[u8; 32]) -> TeeResult<Vec<u8>>;

    /// Unseals a blob previously produced by [`TeePlatform::seal`].
    fn unseal(&self, sealed: &[u8]) -> TeeResult<[u8; 32]>;

    /// Returns the fair-ordering sequencer running inside the trusted boundary.
    ///
    /// This is the existing [`TeeMempool`] reused verbatim — the Nitro backend
    /// does not duplicate the sequencer, it merely hosts it behind the vsock
    /// server.
    fn sequencer(&self) -> Arc<TeeMempool>;
}
