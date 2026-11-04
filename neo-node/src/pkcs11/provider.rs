//! `SignerProvider` implementation backed by a PKCS#11 HSM token.

use crate::config::SignerProviderSection;
use crate::pkcs11::backend::{Pkcs11Backend, Pkcs11KeyQuery, Pkcs11Mechanism, Pkcs11SignCall};
use crate::pkcs11::hsm_pin_from_env;
use crate::signing_provider::NEO_SIGN_DATA_ENCODING;
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use neo_core::signing::{
    ALGORITHM_ECDSA_P256_SHA256, AuthorizedSignRequest, KeyInfo, KeyRef, ProviderCapabilities,
    ProviderOutcomeProvenance, ProviderSignature, SIGNING_SCHEMA_VERSION, SignError, SignErrorCode,
    SignPayload, SignPurpose, SignerProvider,
};
use neo_crypto::Crypto;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use zeroize::Zeroizing;

/// One configured `KeyRef` bound to the HSM key material.
struct HsmKeyBinding {
    account: String,
    public_key: Vec<u8>,
}

/// Signer whose private key never leaves a PKCS#11 token.
///
/// Construction is fail-closed: the backend must expose a digest-capable
/// ECDSA mechanism, the exported public key must match the configured
/// binding, and the user PIN must be available. Every signing call re-checks
/// the exact `KeyRef` (key id and immutable version) and the authorized
/// purpose before touching the token.
pub(crate) struct HsmSigner {
    backend: Arc<dyn Pkcs11Backend>,
    mechanism: Pkcs11Mechanism,
    slot_id: u64,
    token_label: String,
    key_label: String,
    pin: Zeroizing<Vec<u8>>,
    capabilities: ProviderCapabilities,
    keys: BTreeMap<KeyRef, HsmKeyBinding>,
}

impl HsmSigner {
    /// Builds the signer from a validated provider section and backend.
    pub(crate) fn new(
        provider: &SignerProviderSection,
        backend: Arc<dyn Pkcs11Backend>,
        pin: Zeroizing<Vec<u8>>,
    ) -> Result<Self> {
        let pkcs11 = provider.pkcs11.as_ref().with_context(|| {
            format!(
                "hardware TEE signer provider '{}' requires a pkcs11 configuration section",
                provider.id
            )
        })?;
        if pin.as_slice().is_empty() {
            bail!(
                "hardware TEE signer provider '{}' requires a non-empty HSM user PIN",
                provider.id
            );
        }
        if provider.keys.len() != 1 {
            bail!(
                "hardware TEE signer provider '{}' must bind exactly one HSM key so the KeyRef slot, key label, and version stay unambiguous",
                provider.id
            );
        }
        let mechanism = if backend.supports_mechanism(pkcs11.slot_id, Pkcs11Mechanism::EcdsaSha256)
        {
            Pkcs11Mechanism::EcdsaSha256
        } else if backend.supports_mechanism(pkcs11.slot_id, Pkcs11Mechanism::Ecdsa) {
            Pkcs11Mechanism::Ecdsa
        } else {
            bail!(
                "PKCS#11 backend for signer provider '{}' exposes neither CKM_ECDSA_SHA256 nor CKM_ECDSA",
                provider.id
            );
        };
        let mut keys = BTreeMap::new();
        for key in &provider.keys {
            let configured = hex::decode(&key.public_key_hex).with_context(|| {
                format!(
                    "signer provider '{}' key '{}' has invalid public key",
                    provider.id, key.key_id
                )
            })?;
            let exported = backend
                .export_public_key(&Pkcs11KeyQuery {
                    slot_id: pkcs11.slot_id,
                    token_label: pkcs11.token_label.clone(),
                    key_label: pkcs11.key_label.clone(),
                })
                .with_context(|| {
                    format!(
                        "failed to export the HSM public key for signer provider '{}'",
                        provider.id
                    )
                })?;
            if configured.len() != 33 || configured != exported {
                bail!(
                    "HSM-exported public key for signer provider '{}' key '{}' does not match the configured binding",
                    provider.id,
                    key.key_id
                );
            }
            keys.insert(
                KeyRef::new(&provider.id, &key.key_id, key.version),
                HsmKeyBinding {
                    account: key.account.clone(),
                    public_key: exported,
                },
            );
        }
        Ok(Self {
            backend,
            mechanism,
            slot_id: pkcs11.slot_id,
            token_label: pkcs11.token_label.clone(),
            key_label: pkcs11.key_label.clone(),
            pin,
            capabilities: ProviderCapabilities {
                provider: provider.id.clone(),
                purposes: provider.purposes.iter().cloned().collect(),
                canonical_encodings: provider.canonical_encodings.iter().cloned().collect(),
                algorithms: provider.algorithms.iter().cloned().collect(),
                security_level: provider
                    .security_level
                    .expect("validated signer provider security level"),
                outcome_provenance: ProviderOutcomeProvenance::DeterministicLocal,
            },
            keys,
        })
    }

    /// Builds the signer with the PIN read from [`NEO_HSM_PIN_ENV`].
    pub(crate) fn from_env_pin(
        provider: &SignerProviderSection,
        backend: Arc<dyn Pkcs11Backend>,
    ) -> Result<Self> {
        let pin = hsm_pin_from_env()?;
        Self::new(provider, backend, pin)
    }

    fn key_binding(&self, key_ref: &KeyRef) -> neo_core::signing::SignResult<&HsmKeyBinding> {
        self.keys.get(key_ref).ok_or_else(|| {
            SignError::new(
                SignErrorCode::KeyNotFound,
                "HSM signer key id and version are not configured for this provider",
            )
        })
    }

    fn purpose_permitted(&self, purpose: &SignPurpose) -> bool {
        self.capabilities.purposes.contains(purpose)
    }
}

/// Debug renders metadata only; the HSM user PIN is deliberately redacted.
impl fmt::Debug for HsmSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HsmSigner")
            .field("slot_id", &self.slot_id)
            .field("token_label", &self.token_label)
            .field("key_label", &self.key_label)
            .field("mechanism", &self.mechanism)
            .field("configured_keys", &self.keys.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl SignerProvider for HsmSigner {
    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }

    async fn key_info(&self, key_ref: &KeyRef) -> neo_core::signing::SignResult<KeyInfo> {
        let binding = self.key_binding(key_ref)?;
        Ok(KeyInfo {
            schema_version: SIGNING_SCHEMA_VERSION,
            key_ref: key_ref.clone(),
            algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
            public_key: binding.public_key.clone(),
            security_level: self.capabilities.security_level,
            enabled: true,
            attributes: BTreeMap::from([
                ("account".to_string(), binding.account.clone()),
                ("slot_id".to_string(), self.slot_id.to_string()),
                ("token_label".to_string(), self.token_label.clone()),
                ("key_label".to_string(), self.key_label.clone()),
                ("mechanism".to_string(), format!("{:?}", self.mechanism)),
            ]),
        })
    }

    async fn sign_authorized(
        &self,
        request: &AuthorizedSignRequest,
    ) -> neo_core::signing::SignResult<ProviderSignature> {
        let bytes = match request.payload() {
            SignPayload::Canonical { encoding, bytes } if encoding == NEO_SIGN_DATA_ENCODING => {
                bytes
            }
            SignPayload::Canonical { .. } => {
                return Err(SignError::new(
                    SignErrorCode::CapabilityUnavailable,
                    "HSM signer provider rejected an unsupported canonical encoding",
                ));
            }
            SignPayload::Digest { .. } => {
                return Err(SignError::new(
                    SignErrorCode::CapabilityUnavailable,
                    "HSM signer provider does not accept pre-hashed payloads",
                ));
            }
        };
        // Re-check the exact key reference: the immutable key id and version
        // must match the configured HSM key binding.
        self.key_binding(request.key_ref())?;
        // Defense in depth: the service policy authorizes purposes, but the
        // provider independently refuses anything outside its declared scope.
        if !self.purpose_permitted(request.purpose()) {
            return Err(SignError::new(
                SignErrorCode::PolicyDenied,
                "requested purpose is outside the HSM signer provider authorization scope",
            ));
        }
        let data = match self.mechanism {
            Pkcs11Mechanism::EcdsaSha256 => bytes.to_vec(),
            Pkcs11Mechanism::Ecdsa => Crypto::sha256(bytes).to_vec(),
        };
        let signature = self
            .backend
            .sign(&Pkcs11SignCall {
                slot_id: self.slot_id,
                token_label: self.token_label.clone(),
                key_label: self.key_label.clone(),
                pin: self.pin.as_slice().to_vec(),
                mechanism: self.mechanism,
                data,
            })
            .map_err(|error| {
                SignError::new(
                    SignErrorCode::ProviderFailure,
                    format!("PKCS#11 signing failed: {error}"),
                )
            })?;
        if signature.len() != 64 {
            return Err(SignError::new(
                SignErrorCode::ProviderFailure,
                "PKCS#11 backend returned a non-64-byte ECDSA signature",
            ));
        }
        Ok(ProviderSignature {
            algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
            signature,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Pkcs11ProviderSection, SignerKeySection};
    use crate::pkcs11::mock::MockP11Backend;
    use crate::pkcs11::test_env::{pin_env_lock, set_pin};
    use neo_core::cryptography::Crypto;
    use neo_core::signing::{
        AuthenticatedContext, DefaultSignerService, DurableSigningStore, IdempotencyInfo,
        PolicyLimits, ProtocolContext, SignerService, SigningKeyBinding, SigningPolicy,
        SigningStore,
    };
    use neo_core::smart_contract::helper::Helper as ContractHelper;
    use neo_crypto::Secp256r1Crypto;
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::{SystemTime, UNIX_EPOCH};

    const TEST_NETWORK: u32 = 860_833_102;
    const TEST_PIN: &str = "4321";

    fn now_unix_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64
    }

    fn account_for(public_key: &[u8]) -> String {
        let script = ContractHelper::signature_redeem_script(public_key);
        neo_core::UInt160::from(Crypto::hash160(&script)).to_string()
    }

    fn provider_section(
        public_key_hex: &str,
        account: &str,
        purposes: &[SignPurpose],
    ) -> SignerProviderSection {
        SignerProviderSection {
            id: "hsm-tee".to_string(),
            kind: crate::config::SignerProviderKind::HardwareTee,
            enabled: true,
            default: true,
            security_level: Some(neo_core::signing::SecurityLevel::HardwareTee),
            purposes: purposes.to_vec(),
            canonical_encodings: vec![NEO_SIGN_DATA_ENCODING.to_string()],
            algorithms: vec![ALGORITHM_ECDSA_P256_SHA256.to_string()],
            keys: vec![SignerKeySection {
                key_id: "validator-key".to_string(),
                version: 2,
                public_key_hex: public_key_hex.to_string(),
                account: account.to_string(),
                purposes: purposes.to_vec(),
            }],
            endpoint: None,
            remote: None,
            pkcs11: Some(Pkcs11ProviderSection {
                library_path: "unused-mock-library".to_string(),
                slot_id: 0,
                token_label: "TESTTOKEN".to_string(),
                key_label: "validator-key".to_string(),
            }),
        }
    }

    fn pin() -> Zeroizing<Vec<u8>> {
        Zeroizing::new(TEST_PIN.as_bytes().to_vec())
    }

    fn mock_backend(private_key: &[u8; 32]) -> MockP11Backend {
        MockP11Backend::new(0, "TESTTOKEN", "validator-key", private_key, TEST_PIN)
    }

    #[test]
    fn pkcs11_hsm_signer_construction_binds_exported_public_key() {
        let private_key = [0x53; 32];
        let backend = mock_backend(&private_key);
        let section = provider_section(
            &hex::encode(backend.public_key()),
            &account_for(backend.public_key()),
            &[SignPurpose::Consensus],
        );
        let signer =
            HsmSigner::new(&section, Arc::new(backend.clone()), pin()).expect("constructed signer");
        let binding = signer
            .key_binding(&KeyRef::new("hsm-tee", "validator-key", 2))
            .expect("configured KeyRef binding");
        assert_eq!(binding.public_key.len(), 33);
    }

    #[test]
    fn pkcs11_hsm_signer_construction_rejects_public_key_mismatch() {
        let backend = mock_backend(&[0x53; 32]);
        let other_key = Secp256r1Crypto::derive_public_key(&[0x54; 32]).expect("other public key");
        let section = provider_section(
            &hex::encode(&other_key),
            &account_for(&other_key),
            &[SignPurpose::Consensus],
        );
        let error = HsmSigner::new(&section, Arc::new(backend.clone()), pin())
            .expect_err("mismatched public key must fail closed");
        assert!(
            format!("{error:#}").contains("does not match"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn pkcs11_hsm_signer_construction_rejects_missing_key_object() {
        let backend = mock_backend(&[0x53; 32]);
        let mut section = provider_section(
            &hex::encode(backend.public_key()),
            &account_for(backend.public_key()),
            &[SignPurpose::Consensus],
        );
        section.pkcs11.as_mut().expect("pkcs11 section").key_label = "missing-key".to_string();
        let error = HsmSigner::new(&section, Arc::new(backend.clone()), pin())
            .expect_err("unknown key label must fail closed");
        assert!(
            format!("{error:#}").contains("no token matching"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn pkcs11_hsm_signer_construction_rejects_backend_without_mechanism() {
        let backend = mock_backend(&[0x53; 32]).restrict_mechanisms(&[]);
        let section = provider_section(
            &hex::encode(backend.public_key()),
            &account_for(backend.public_key()),
            &[SignPurpose::Consensus],
        );
        let error = HsmSigner::new(&section, Arc::new(backend.clone()), pin())
            .expect_err("no supported mechanism must fail closed");
        assert!(
            format!("{error:#}").contains("neither CKM_ECDSA_SHA256 nor CKM_ECDSA"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn pkcs11_hsm_signer_pin_environment_missing_fails_closed() {
        let backend = mock_backend(&[0x53; 32]);
        let section = provider_section(
            &hex::encode(backend.public_key()),
            &account_for(backend.public_key()),
            &[SignPurpose::Consensus],
        );
        let _guard = pin_env_lock();
        set_pin(None);
        let error = HsmSigner::from_env_pin(&section, Arc::new(backend.clone()))
            .expect_err("missing PIN must fail closed");
        assert!(
            format!("{error:#}").contains(crate::pkcs11::NEO_HSM_PIN_ENV),
            "unexpected error: {error:#}"
        );
    }

    #[tokio::test]
    async fn pkcs11_hsm_signer_key_info_rejects_unknown_key_ref_version() {
        let backend = mock_backend(&[0x53; 32]);
        let section = provider_section(
            &hex::encode(backend.public_key()),
            &account_for(backend.public_key()),
            &[SignPurpose::Consensus],
        );
        let signer =
            HsmSigner::new(&section, Arc::new(backend.clone()), pin()).expect("constructed signer");
        let error = signer
            .key_info(&KeyRef::new("hsm-tee", "validator-key", 3))
            .await
            .expect_err("wrong KeyRef version must be rejected");
        assert_eq!(error.code, SignErrorCode::KeyNotFound);
        assert!(
            error.message.contains("key id and version"),
            "unexpected error: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn pkcs11_hsm_signer_key_info_rejects_unknown_provider_namespace() {
        let backend = mock_backend(&[0x53; 32]);
        let section = provider_section(
            &hex::encode(backend.public_key()),
            &account_for(backend.public_key()),
            &[SignPurpose::Consensus],
        );
        let signer =
            HsmSigner::new(&section, Arc::new(backend.clone()), pin()).expect("constructed signer");
        let error = signer
            .key_info(&KeyRef::new("other-provider", "validator-key", 2))
            .await
            .expect_err("foreign provider namespace must be rejected");
        assert_eq!(error.code, SignErrorCode::KeyNotFound);
    }

    fn signing_context() -> AuthenticatedContext {
        AuthenticatedContext {
            schema_version: SIGNING_SCHEMA_VERSION,
            principal: "consensus-a".to_string(),
            tenant: "tenant-a".to_string(),
            authentication_method: "pkcs11-test".to_string(),
            scopes: BTreeSet::from(["sign:consensus".to_string()]),
            claims: BTreeMap::new(),
        }
    }

    fn signing_request(
        request_id: &str,
        key_ref: KeyRef,
        purpose: SignPurpose,
    ) -> neo_core::signing::SignRequest {
        neo_core::signing::SignRequest {
            schema_version: SIGNING_SCHEMA_VERSION,
            request_id: request_id.to_string(),
            network: TEST_NETWORK,
            purpose,
            key_ref,
            payload: SignPayload::Canonical {
                encoding: NEO_SIGN_DATA_ENCODING.to_string(),
                bytes: vec![9, 8, 7, 6, 5, 4, 3, 2, 1],
            },
            protocol_context: ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: "neo-n3-consensus".to_string(),
                protocol_version: "1".to_string(),
                fields: BTreeMap::new(),
            },
            policy_version: "policy-v1".to_string(),
            deadline_unix_ms: now_unix_ms().saturating_add(60_000),
            idempotency: IdempotencyInfo {
                schema_version: SIGNING_SCHEMA_VERSION,
                key: format!("idem-{request_id}"),
                replay_nonce: format!("nonce-{request_id}"),
            },
            fee_units: 1,
            quota_units: 1,
            account: Some("account-a".to_string()),
            targets: BTreeSet::new(),
        }
    }

    fn signing_policy(key_ref: KeyRef, key_public: Vec<u8>) -> SigningPolicy {
        let mut policy = SigningPolicy {
            version: "policy-v1".to_string(),
            key_resolver: neo_core::signing::SigningKeyResolver::new([SigningKeyBinding {
                account: "account-a".to_string(),
                purpose: SignPurpose::Consensus,
                key_ref: key_ref.clone(),
                public_key: key_public,
            }])
            .expect("key resolver"),
            limits: PolicyLimits {
                max_fee_units: 10,
                quota_limit_units: 10,
                quota_window_ms: 60_000,
            },
            ..SigningPolicy::default()
        };
        policy.principals.allow.insert("consensus-a".to_string());
        policy.tenants.allow.insert("tenant-a".to_string());
        policy.keys.allow.insert(key_ref.selector());
        policy.networks.allow.insert(TEST_NETWORK);
        policy.purposes.allow.insert(SignPurpose::Consensus);
        policy
            .authorizations
            .allow
            .insert(SigningPolicy::authorization_selector(
                "consensus-a",
                "tenant-a",
                &key_ref,
                TEST_NETWORK,
                &SignPurpose::Consensus,
                "sign:consensus",
            ));
        policy
            .security_levels
            .allow
            .insert(neo_core::signing::SecurityLevel::HardwareTee);
        policy.accounts.allow.insert("account-a".to_string());
        policy
    }

    async fn service_with(
        backend: MockP11Backend,
        purposes: &[SignPurpose],
    ) -> (Arc<DefaultSignerService>, MockP11Backend) {
        let key_ref = KeyRef::new("hsm-tee", "validator-key", 2);
        let public_key = backend.public_key().to_vec();
        let section = provider_section(
            &hex::encode(&public_key),
            &account_for(&public_key),
            purposes,
        );
        let signer: Arc<dyn SignerProvider> =
            Arc::new(HsmSigner::new(&section, Arc::new(backend.clone()), pin()).expect("signer"));
        let directory = tempfile::tempdir().expect("temporary signing store");
        let store: Arc<dyn SigningStore> = Arc::new(
            DurableSigningStore::open(directory.path().join("signing.journal"))
                .expect("durable signing store"),
        );
        let service = Arc::new(DefaultSignerService::new(
            signer,
            store,
            Arc::new(signing_policy(key_ref, public_key)),
        ));
        (service, backend)
    }

    #[tokio::test]
    async fn pkcs11_hsm_signer_signs_with_negotiated_mechanism_and_canonical_bytes() {
        let backend = mock_backend(&[0x53; 32]);
        let (service, backend) = service_with(backend, &[SignPurpose::Consensus]).await;
        let request = signing_request(
            "pkcs11-consensus-1",
            KeyRef::new("hsm-tee", "validator-key", 2),
            SignPurpose::Consensus,
        );
        let payload_bytes = request
            .payload
            .canonical_bytes()
            .expect("canonical bytes")
            .to_vec();
        let response = service
            .sign(request, signing_context())
            .await
            .expect("authorized consensus signing succeeds");
        assert_eq!(response.algorithm, ALGORITHM_ECDSA_P256_SHA256);
        assert_eq!(response.signature.len(), 64);
        let raw: [u8; 64] = response
            .signature
            .as_slice()
            .try_into()
            .expect("raw signature");
        assert!(
            Secp256r1Crypto::verify(&payload_bytes, &raw, backend.public_key())
                .expect("verify signature"),
            "signature must verify against the HSM public key"
        );
        let calls = backend.recorded_signs();
        assert_eq!(calls.len(), 1, "exactly one backend sign call");
        assert_eq!(calls[0].mechanism, Pkcs11Mechanism::EcdsaSha256);
        assert_eq!(calls[0].data, payload_bytes);
        assert_eq!(calls[0].pin, TEST_PIN.as_bytes());
        assert_eq!(calls[0].slot_id, 0);
        assert_eq!(calls[0].token_label, "TESTTOKEN");
        assert_eq!(calls[0].key_label, "validator-key");
    }

    #[tokio::test]
    async fn pkcs11_hsm_signer_falls_back_to_ckm_ecdsa_with_sha256_digest() {
        let backend = mock_backend(&[0x53; 32]).restrict_mechanisms(&[Pkcs11Mechanism::Ecdsa]);
        let (service, backend) = service_with(backend, &[SignPurpose::Consensus]).await;
        let request = signing_request(
            "pkcs11-consensus-2",
            KeyRef::new("hsm-tee", "validator-key", 2),
            SignPurpose::Consensus,
        );
        let payload_bytes = request
            .payload
            .canonical_bytes()
            .expect("canonical bytes")
            .to_vec();
        let response = service
            .sign(request, signing_context())
            .await
            .expect("CKM_ECDSA digest signing succeeds");
        let raw: [u8; 64] = response
            .signature
            .as_slice()
            .try_into()
            .expect("raw signature");
        assert!(
            Secp256r1Crypto::verify(&payload_bytes, &raw, backend.public_key())
                .expect("verify signature"),
            "digest-based signature must verify against the HSM public key"
        );
        let calls = backend.recorded_signs();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].mechanism, Pkcs11Mechanism::Ecdsa);
        assert_eq!(calls[0].data, Crypto::sha256(&payload_bytes).to_vec());
    }

    #[tokio::test]
    async fn pkcs11_hsm_signer_rejects_purpose_outside_authorization_scope() {
        let backend = mock_backend(&[0x53; 32]);
        let (service, backend) = service_with(backend, &[SignPurpose::Consensus]).await;
        // The provider declares only consensus; a state-root request must be
        // denied before any backend call is made.
        let mut request = signing_request(
            "pkcs11-stateroot-denied",
            KeyRef::new("hsm-tee", "validator-key", 2),
            SignPurpose::Consensus,
        );
        request.purpose = SignPurpose::StateRoot;
        let error = service
            .sign(request, signing_context())
            .await
            .expect_err("out-of-scope purpose must be denied");
        assert!(
            error.code == SignErrorCode::PolicyDenied
                || error.code == SignErrorCode::CapabilityUnavailable,
            "unexpected error code {:?}: {}",
            error.code,
            error.message
        );
        assert!(backend.recorded_signs().is_empty());
    }

    #[tokio::test]
    async fn pkcs11_hsm_signer_rejects_key_ref_version_mismatch_at_signing() {
        let backend = mock_backend(&[0x53; 32]);
        let (service, backend) = service_with(backend, &[SignPurpose::Consensus]).await;
        let request = signing_request(
            "pkcs11-version-mismatch",
            KeyRef::new("hsm-tee", "validator-key", 3),
            SignPurpose::Consensus,
        );
        let error = service
            .sign(request, signing_context())
            .await
            .expect_err("wrong key version must be rejected");
        assert_eq!(error.code, SignErrorCode::KeyNotFound);
        assert!(backend.recorded_signs().is_empty());
    }

    #[test]
    fn pkcs11_hsm_signer_rejects_payload_digest_outside_provider() {
        let backend = mock_backend(&[0x53; 32]);
        let public_key = backend.public_key().to_vec();
        let section = provider_section(
            &hex::encode(&public_key),
            &account_for(&public_key),
            &[SignPurpose::Consensus],
        );
        let signer =
            HsmSigner::new(&section, Arc::new(backend.clone()), pin()).expect("constructed signer");
        let capabilities = signer.capabilities();
        assert!(
            !capabilities.supports(
                &SignPurpose::Consensus,
                &SignPayload::Digest {
                    algorithm: neo_core::signing::DigestAlgorithm::Sha256,
                    digest: [7; 32],
                },
                ALGORITHM_ECDSA_P256_SHA256,
            ),
            "digest payloads must stay outside the declared canonical encodings"
        );
    }
}
