//! # neo-tee::nitro
//!
//! AWS Nitro enclave integration helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-tee`. This adapter crate owns TEE integration
//! and must not define protocol bytes, consensus rules, or storage semantics.
//!
//! ## Contents
//!
//! - `attestation`: TEE attestation evidence and verification helpers.
//! - `ordering`: Nitro ordering helpers.
//! - `platform`: Nitro platform adapter.
//! - `signer`: signer configuration and signing helpers.
//! - `vsock`: Nitro vsock transport adapter.

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
