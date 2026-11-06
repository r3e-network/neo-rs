//! Reference implementation of the dedicated remote TEE signer service.
//!
//! # What this is
//!
//! `neo-signer-server` implements the server side of the wire protocol defined
//! in [`neo_signing::remote`] exactly as the node-side
//! `neo-node` remote signer transport and provider expect it:
//!
//! * `POST /v1/signer/key-info` — authenticated key metadata
//!   ([`neo_signing::RemoteKeyInfoResponse`]).
//! * `POST /v1/signer/attest` — challenge-bound attestation evidence
//!   ([`neo_signing::AttestationEvidence`]) generated with the
//!   [`neo_tee`] `AttestationService` in **simulation** mode, binding the
//!   challenge nonce, the per-session P-256 public key, the service identity,
//!   the policy version and the [`neo_signing::KeyRef`] via
//!   [`AttestationEvidence::binding_digest`].
//! * `POST /v1/signer/sign` — authorized signing. Success responds `200` with
//!   an authenticated [`neo_signing::RemoteSignResponse`]; a permanent
//!   policy denial responds with a 4xx status carrying an authenticated
//!   [`neo_signing::RemoteSignRejection`]; anything ambiguous (the
//!   operation may or may not have completed) responds with a 5xx/429 status
//!   so the node classifies it as `ResultUnknown` and recovers through the
//!   query endpoint.
//! * `POST /v1/signer/query` — idempotent recovery of a previously dispatched
//!   operation ([`neo_signing::RemoteOperationStatus`]). Requests are
//!   journaled by request id: a completed request is replayed byte-for-byte
//!   and is never re-signed.
//!
//! The transport is HTTPS with mutual TLS only (the TLS listener rejects any
//! non-TLS connection at handshake time and requires a client certificate
//! issued by the configured CA). The signing key is sealed at first boot with
//! [`neo_tee::SealedKey`] and re-unsealed on later boots, inheriting the
//! rollback-floor protection of the neo-tee enclave.
//!
//! # What this is NOT
//!
//! **This is a reference implementation for tests, staging and protocol
//! bring-up.** Its attestation evidence is produced by the neo-tee
//! *simulation* enclave. Simulation evidence does NOT satisfy a
//! `RemoteHardwareTee` policy in production: the node client's pluggable
//! `RemoteAttestationVerifier` must only accept quotes verified
//! cryptographically against real hardware collateral. A production
//! deployment must:
//!
//! 1. run the signing path inside a real hardware TEE (SGX/TDX/SEV-SNP),
//! 2. produce hardware quotes bound to the session binding digest, and
//! 3. install a verifier that validates quote signatures, TCB and revocation
//!    collateral before trusting any claim.
//!
//! The fault-injection switches in [`config::FaultPolicy`] exist exclusively
//! so integration tests can exercise the node client's rejection and
//! recovery paths; they must never be enabled in production.

pub mod config;
pub mod http;
pub mod keys;
pub mod protocol;

pub use config::{FaultPolicy, SignerServerConfig, TlsPaths};
pub use http::{SignerServerHandle, spawn_signer_server};
pub use keys::SealedSigningKey;
pub use protocol::{SignOutcome, SignerService};
