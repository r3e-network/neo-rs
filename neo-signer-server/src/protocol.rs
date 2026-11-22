//! Core signing protocol: endpoint semantics of the reference signer service.
//!
//! This module owns the behavior the node-side `RemoteSignerProvider`
//! validates:
//!
//! * attestation challenges are single-use and freshness-bounded;
//! * evidence binds the challenge nonce, session P-256 key, service identity,
//!   policy version and [`KeyRef`] into the report data of a neo-tee
//!   (simulation) attestation report;
//! * every response and permanent rejection is authenticated by a P-256
//!   signature of the attested session key over
//!   [`RemoteSignResponse::auth_digest`] / [`RemoteSignRejection::auth_digest`];
//! * every dispatch is journaled by request id in an **append-only file inside
//!   the sealed-data directory** — completed requests are replayed, never
//!   re-signed, survive a restart, and are recoverable through the query
//!   endpoint (`Pending`/`Succeeded`/`Failed`/`NotFound`);
//! * consensus requests reserve their `(network, height, view, phase)` slot
//!   durably, and a conflicting request for an already reserved slot is
//!   permanently rejected (anti-equivocation);
//! * every endpoint is scoped to the authenticated mutual-TLS client identity:
//!   the [`crate::config::ClientAuthorizationPolicy`] maps certificate common
//!   names to the key IDs and purposes they may use.

use crate::config::{ClientAuthorizationPolicy, SignerServerConfig, key_selector};
use crate::identity::ClientIdentity;
use crate::journal::{ConsensusSlot, Journal, JournalState};
use crate::keys::SealedSigningKey;
use anyhow::Context;
use neo_crypto::Secp256r1Crypto;
use neo_tee::attestation::AttestationConfig;
use neo_tee::{AttestationService, TeeEnclave};
use neo_wallets::signing::{
    ALGORITHM_ECDSA_P256_SHA256, AttestationChallenge, AttestationEvidence, KeyInfo, KeyRef,
    ProviderSignature, REMOTE_SIGNER_PROTOCOL_VERSION, RemoteKeyInfoResponse, RemoteOperationQuery,
    RemoteOperationStatus, RemoteSignRejection, RemoteSignRequest, RemoteSignResponse,
    SecurityLevel, SignError, SignErrorCode, SignPurpose, SignResult, now_unix_ms,
    validate_remote_schema,
};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use zeroize::Zeroizing;

/// Permanent rejection codes accepted by the node client's
/// `is_permanent_rejection_code` check (mirrored here for documentation).
pub const PERMANENT_REJECTION_CODES: &[&str] = &[
    "authorization_denied",
    "key_disabled",
    "key_not_found",
    "policy_permanent_denial",
];

/// One live attested session. The private key authenticates every response
/// and rejection bound to this session.
#[derive(Clone)]
struct SessionRecord {
    session_id: String,
    /// Session key. Held in [`Zeroizing`] so the private scalar is wiped when
    /// the session expires, is replaced, or the service is dropped.
    session_private_key: Zeroizing<[u8; 32]>,
    key_ref: KeyRef,
    /// Common name of the mutual-TLS client that established the session. A
    /// session is never usable by another identity.
    client_common_name: String,
    expires_at_unix_ms: i64,
}

impl SessionRecord {
    fn is_valid_at(&self, now: i64, key_ref: &KeyRef, client: &ClientIdentity) -> bool {
        now < self.expires_at_unix_ms
            && &self.key_ref == key_ref
            && self.client_common_name == client.common_name()
    }
}

/// Outcome of one signing dispatch, mapped onto HTTP statuses by the HTTP
/// layer:
///
/// * [`SignOutcome::Succeeded`] → `200` with a [`RemoteSignResponse`],
/// * [`SignOutcome::Rejected`] → 4xx with an authenticated
///   [`RemoteSignRejection`] (terminal for the node client),
/// * [`SignOutcome::Unauthorized`] → `403` when the authenticated client
///   identity is not scoped to the requested key or purpose,
/// * [`SignOutcome::Ambiguous`] → 5xx, so the node classifies the dispatch as
///   `ResultUnknown` and recovers through the query endpoint.
pub enum SignOutcome {
    Succeeded(RemoteSignResponse),
    Rejected(RemoteSignRejection),
    Unauthorized(String),
    Ambiguous(String),
}

/// The signer service. All endpoint logic lives here; the HTTP layer is a
/// thin translation shim.
pub struct SignerService {
    config: SignerServerConfig,
    key: SealedSigningKey,
    attestation: AttestationService,
    authorization: ClientAuthorizationPolicy,
    sessions: Mutex<BTreeMap<String, SessionRecord>>,
    used_challenges: Mutex<BTreeMap<[u8; 32], i64>>,
    /// Durable, append-only operation journal. Restores on startup, so a
    /// restart can neither forget a completed operation nor forget a reserved
    /// consensus slot.
    journal: Mutex<Journal>,
    signatures_issued: AtomicUsize,
    attestation_sessions_issued: AtomicUsize,
}

mod service;

#[cfg(test)]
mod tests;
