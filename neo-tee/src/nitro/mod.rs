//! AWS Nitro Enclaves backend (EXPERIMENTAL, feature-gated).
//!
//! This module is an **experimental skeleton** for running a Neo N3 validator's
//! signing key and fair-ordering sequencer inside an AWS Nitro Enclave. It is
//! gated behind the `nitro` cargo feature and is **off by default**. It is NOT
//! wired into `neo-node`; selecting a Nitro signer is a deliberate, separate
//! node-level change.
//!
//! Design reference: `claudedocs/aws-hsm-nitro-tee-design.md` (§2 Architecture,
//! §3.2 module plan, §4 integration).
//!
//! # Module map
//!
//! | Module | Responsibility |
//! | --- | --- |
//! | [`platform`] | [`platform::TeePlatform`] seam shared by Nitro / future SGX. |
//! | [`attestation`] | NSM document model + pure COSE/CBOR parser + structural validation. |
//! | [`vsock`] | host<->enclave wire types, length-framed codec, transports. |
//! | [`signer`] | [`signer::NitroEnclaveSigner`] implementing `ConsensusSigner`. |
//! | [`ordering`] | verifiable fair-ordering proofs over the existing sequencer. |
//!
//! # What is real vs experimental
//!
//! REAL + tested (no hardware needed):
//! - The COSE_Sign1 / CBOR attestation parser and structural validation.
//! - The vsock wire types and length-framed codec.
//! - The `NitroEnclaveSigner` `sign`/`can_sign` logic over a transport.
//! - Ordering-proof construction (reusing the existing sequencer) and the
//!   internal-consistency verifier (merkle root + sequencer signature).
//!
//! EXPERIMENTAL — every site below is marked
//! `// EXPERIMENTAL: validate in a Nitro environment before production`:
//! - Real AF_VSOCK connect/transport ([`vsock::RealVsockTransport`]).
//! - NSM ioctl-based attestation generation (not present; belongs in the
//!   enclave binary which requires the NSM driver).
//! - COSE ES384 signature + X.509 chain verification to the pinned Nitro Root
//!   G1 ([`attestation::verify_pki_chain`]).
//! - KMS attested-decrypt key import (not implemented; design doc §5.2).

pub mod attestation;
pub mod ordering;
pub mod platform;
pub mod signer;
pub mod vsock;

pub use attestation::{
    CoseSign1, NITRO_ROOT_G1_SHA256_FINGERPRINT, NitroAttestationDoc, NitroValidationOptions,
    PkiVerification, parse_cose_sign1, verify_pki_chain,
};
pub use ordering::{
    OrderingProof, OrderingVerification, build_ordering_proof, verify_ordering_proof,
};
pub use platform::{PlatformKind, TeePlatform};
pub use signer::{NitroEnclaveSigner, script_hash_from_public_key};
pub use vsock::{
    EnclaveRequest, EnclaveResponse, MAX_FRAME_LEN, MockTransport, OrderTxEntry, PROTOCOL_VERSION,
    RealVsockTransport, VsockAddr, VsockTransport, decode_frame, encode_frame,
};
