//! Dedicated mTLS remote signer and remotely attested TEE adapter.

use super::config::{RemoteAttestationPolicySection, SignerProviderKind, SignerProviderSection};
use super::remote::now_unix_ms;
use crate::signing::{
    ALGORITHM_ECDSA_P256_SHA256, AttestationChallenge, AttestationEvidence,
    AttestationExpectations, AuthorizedSignRequest, CollateralStatus, KeyInfo, KeyRef,
    ProviderCapabilities, ProviderOperationStatus, ProviderOutcomeProvenance, ProviderSignature,
    REMOTE_SIGNER_PROTOCOL_VERSION, RemoteAttestationVerifier, RemoteKeyInfoResponse,
    RemoteOperationQuery, RemoteOperationStatus, RemoteSignDispatchResult, RemoteSignRejection,
    RemoteSignRequest, RemoteSignResponse, RemoteSignerTransport, SignError, SignErrorCode,
    SignResult, SignerProvider, TcbStatus, VerifiedAttestation, validate_remote_schema,
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use neo_crypto::Secp256r1Crypto;
use parking_lot::RwLock;
use rand::{RngCore, rngs::OsRng};
use reqwest::{Certificate, Client, Identity, StatusCode, Url, redirect::Policy};
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::Arc;
use std::time::Duration;

const KEY_INFO_PATH: &str = "v1/signer/key-info";
const ATTEST_PATH: &str = "v1/signer/attest";
const SIGN_PATH: &str = "v1/signer/sign";
const QUERY_PATH: &str = "v1/signer/query";
const PERMANENT_REJECTION_CODES: &[&str] = &[
    "authorization_denied",
    "key_disabled",
    "key_not_found",
    "policy_permanent_denial",
];
pub(crate) const MAX_ATTESTATION_CLOCK_SKEW_MS: u64 = 60_000;

fn is_permanent_rejection_code(code: &str) -> bool {
    PERMANENT_REJECTION_CODES.contains(&code)
}

fn provider_error(message: impl Into<String>) -> SignError {
    SignError::new(SignErrorCode::ProviderFailure, message)
}

pub fn validate_remote_provider_config(provider: &SignerProviderSection) -> Result<()> {
    let endpoint = provider
        .endpoint
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "remote signer provider '{}' requires an HTTPS endpoint",
                provider.id
            )
        })?;
    let url = Url::parse(endpoint).with_context(|| {
        format!(
            "remote signer provider '{}' has an invalid endpoint",
            provider.id
        )
    })?;
    if url.scheme() != "https"
        || url.username() != ""
        || url.password().is_some()
        || url.host_str().is_none()
    {
        bail!(
            "remote signer provider '{}' endpoint must be credential-free HTTPS",
            provider.id
        );
    }
    let remote = provider.remote.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "remote signer provider '{}' requires dedicated mTLS configuration",
            provider.id
        )
    })?;
    if remote.server_name.trim().is_empty()
        || url.host_str() != Some(remote.server_name.trim())
        || remote.ca_certificate_path.trim().is_empty()
        || remote.client_certificate_path.trim().is_empty()
        || remote.client_private_key_path.trim().is_empty()
        || remote.service_identity.trim().is_empty()
        || provider.keys.is_empty()
        || remote.identity_mapping.get(&remote.service_identity) != Some(&provider.id)
    {
        bail!(
            "remote signer provider '{}' requires matching service name, mTLS files, identity mapping, and keys",
            provider.id
        );
    }
    for path in [
        &remote.ca_certificate_path,
        &remote.client_certificate_path,
        &remote.client_private_key_path,
    ] {
        let metadata = fs::metadata(path).with_context(|| {
            format!(
                "remote signer provider '{}' cannot read TLS material '{path}'",
                provider.id
            )
        })?;
        if !metadata.is_file() {
            bail!(
                "remote signer provider '{}' TLS material '{path}' is not a file",
                provider.id
            );
        }
    }
    let mut keys = BTreeSet::new();
    for key in &provider.keys {
        let public_key = hex::decode(&key.public_key_hex).with_context(|| {
            format!(
                "remote signer provider '{}' key '{}' has invalid public_key_hex",
                provider.id, key.key_id
            )
        })?;
        if key.key_id.trim().is_empty()
            || key.version == 0
            || public_key.len() != 33
            || !keys.insert((key.key_id.as_str(), key.version))
        {
            bail!(
                "remote signer provider '{}' requires unique versioned keys and compressed P-256 public keys",
                provider.id
            );
        }
    }
    Ok(())
}

mod provider;
mod session;
mod transport;

pub use provider::RemoteSignerProvider;
pub use transport::MtlsRemoteSignerTransport;

#[cfg(all(test, feature = "remote"))]
mod tests;
