// `dedup` and `filter_json` test suites moved with the engine into the
// `neo-oracle` crate (see `neo-oracle/src/service/tests.rs`); `response_tx`
// stays here because the fee/witness parity assertions pin the neo-core
// `CoreResponseTxBuilder` adapter against the C# values.
mod response_tx;

use super::{OracleService, OracleServiceSettings};
use async_trait::async_trait;
use neo_wallets::KeyPair;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_signing::{
    AuthenticatedContext, FrozenSignIntent, KeyRef, SIGNING_SCHEMA_VERSION, SignPurpose,
    SignRequest, SignResponse, SignResult, SignerService, SigningKeyBinding, SigningKeyResolver,
    TrustedSignerClient,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct RecordingOracleSigner {
    registered_key_ref: KeyRef,
    requests: Mutex<Vec<(SignRequest, AuthenticatedContext)>>,
    resume_calls: AtomicUsize,
}

#[async_trait]
impl SignerService for RecordingOracleSigner {
    async fn sign(
        &self,
        request: SignRequest,
        context: AuthenticatedContext,
    ) -> SignResult<SignResponse> {
        let request_digest = request.binding_digest(&context)?;
        self.requests
            .lock()
            .expect("oracle recording lock")
            .push((request.clone(), context));
        if request.key_ref != self.registered_key_ref {
            return Err(neo_signing::SignError::new(
                neo_signing::SignErrorCode::KeyNotFound,
                "oracle signer key is not registered",
            ));
        }
        let response = SignResponse {
            schema_version: SIGNING_SCHEMA_VERSION,
            request_id: request.request_id.clone(),
            key_ref: request.key_ref.clone(),
            algorithm: neo_signing::ALGORITHM_ECDSA_P256_SHA256.to_string(),
            signature: vec![0x4F; 64],
            signed_digest: request.payload.signing_digest(),
            request_digest,
            policy_version: request.policy_version.clone(),
            completed_at_unix_ms: 1,
            idempotent_replay: false,
        };
        Ok(response)
    }

    async fn resume_or_start(
        &self,
        operation_id: &str,
        frozen_intent: FrozenSignIntent,
        context: AuthenticatedContext,
        request_timeout: Duration,
    ) -> SignResult<SignResponse> {
        self.resume_calls.fetch_add(1, Ordering::SeqCst);
        self.sign(
            frozen_intent.into_request(
                operation_id,
                request_timeout.as_millis().min(i64::MAX as u128) as i64,
            )?,
            context,
        )
        .await
    }
}

#[tokio::test]
async fn oracle_uses_configured_non_default_key_binding() {
    let protocol = ProtocolSettings::testnet();
    let network = protocol.network;
    let system = crate::neo_system::NeoSystem::new(protocol, None, None).expect("neo system");
    let service = OracleService::new(
        OracleServiceSettings {
            network,
            ..Default::default()
        },
        system,
    )
    .expect("oracle service");
    let key_pair = KeyPair::from_private_key(&[0x41; 32]).expect("oracle key");
    let account = key_pair.get_script_hash();
    let configured = KeyRef::new("oracle-hsm", "oracle-key-v6", 6);
    let resolver = Arc::new(
        SigningKeyResolver::new([SigningKeyBinding {
            account: account.to_string(),
            purpose: SignPurpose::OracleResponse,
            key_ref: configured.clone(),
            public_key: key_pair.public_key(),
        }])
        .expect("oracle key resolver"),
    );
    let recording = Arc::new(RecordingOracleSigner {
        registered_key_ref: configured.clone(),
        requests: Mutex::new(Vec::new()),
        resume_calls: AtomicUsize::new(0),
    });
    let signer = TrustedSignerClient::new(
        recording.clone(),
        "software-default",
        "oracle-policy-v5",
        Duration::from_secs(5),
        AuthenticatedContext {
            schema_version: SIGNING_SCHEMA_VERSION,
            principal: "oracle-test".to_string(),
            tenant: "node-test".to_string(),
            authentication_method: "local-test".to_string(),
            scopes: BTreeSet::from(["sign:oracle-response".to_string()]),
            claims: BTreeMap::new(),
        },
        resolver,
    )
    .expect("trusted oracle signer");
    service.set_signer_client(signer);

    let signature = service
        .sign_neo_payload(
            "oracle-operation".to_string(),
            account.clone(),
            vec![1, 3, 5, 7],
            "neo-n3-oracle-response-submission",
            BTreeMap::from([("request_id".to_string(), "7".to_string())]),
        )
        .await
        .expect("oracle signature");
    assert_eq!(signature, vec![0x4F; 64]);
    let requests = recording.requests.lock().expect("oracle recording lock");
    assert_eq!(requests.len(), 1);
    let (request, context) = &requests[0];
    assert_eq!(request.schema_version, SIGNING_SCHEMA_VERSION);
    assert_eq!(request.key_ref, configured);
    assert_eq!(request.key_ref.provider, "oracle-hsm");
    assert_eq!(request.key_ref.key_id, "oracle-key-v6");
    assert_eq!(request.key_ref.version, 6);
    assert_ne!(
        request.key_ref,
        KeyRef::new("software-default", "default-key", 1)
    );
    assert_eq!(request.purpose, SignPurpose::OracleResponse);
    assert_eq!(request.policy_version, "oracle-policy-v5");
    assert_eq!(
        context.scopes,
        BTreeSet::from(["sign:oracle-response".to_string()])
    );
    drop(requests);
    assert_eq!(recording.resume_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn oracle_rejects_key_version_mismatch_without_default_fallback() {
    let protocol = ProtocolSettings::testnet();
    let network = protocol.network;
    let system = crate::neo_system::NeoSystem::new(protocol, None, None).expect("neo system");
    let service = OracleService::new(
        OracleServiceSettings {
            network,
            ..Default::default()
        },
        system,
    )
    .expect("oracle service");
    let key_pair = KeyPair::from_private_key(&[0x42; 32]).expect("oracle key");
    let account = key_pair.get_script_hash();
    let configured = KeyRef::new("oracle-hsm", "oracle-key-v6", 6);
    let mismatched = KeyRef::new("oracle-hsm", "oracle-key-v6", 7);
    let default_fallback = KeyRef::new("software-default", "default-key", 1);
    let resolver = Arc::new(
        SigningKeyResolver::new([SigningKeyBinding {
            account: account.to_string(),
            purpose: SignPurpose::OracleResponse,
            key_ref: mismatched.clone(),
            public_key: key_pair.public_key(),
        }])
        .expect("mismatched oracle key resolver"),
    );
    let recording = Arc::new(RecordingOracleSigner {
        registered_key_ref: configured.clone(),
        requests: Mutex::new(Vec::new()),
        resume_calls: AtomicUsize::new(0),
    });
    let signer = TrustedSignerClient::new(
        recording.clone(),
        "software-default",
        "oracle-policy-v5",
        Duration::from_secs(5),
        AuthenticatedContext {
            schema_version: SIGNING_SCHEMA_VERSION,
            principal: "oracle-test".to_string(),
            tenant: "node-test".to_string(),
            authentication_method: "local-test".to_string(),
            scopes: BTreeSet::from(["sign:oracle-response".to_string()]),
            claims: BTreeMap::new(),
        },
        resolver,
    )
    .expect("trusted oracle signer");
    service.set_signer_client(signer);

    let error = service
        .sign_neo_payload(
            "oracle-operation-version-mismatch".to_string(),
            account,
            vec![1, 3, 5, 7],
            "neo-n3-oracle-response-submission",
            BTreeMap::from([("request_id".to_string(), "8".to_string())]),
        )
        .await
        .expect_err("version mismatch must fail closed");
    assert!(error.to_string().contains("not registered"));
    let requests = recording.requests.lock().expect("oracle recording lock");
    assert_eq!(requests.len(), 1);
    let (request, context) = &requests[0];
    assert_eq!(request.key_ref, mismatched);
    assert_eq!(request.key_ref.version, 7);
    assert_ne!(request.key_ref, configured);
    assert_ne!(request.key_ref, default_fallback);
    assert_eq!(request.purpose, SignPurpose::OracleResponse);
    assert_eq!(request.policy_version, "oracle-policy-v5");
    assert_eq!(
        context.scopes,
        BTreeSet::from(["sign:oracle-response".to_string()])
    );
    assert_eq!(recording.resume_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn oracle_rejects_unknown_provider_without_default_fallback() {
    let protocol = ProtocolSettings::testnet();
    let network = protocol.network;
    let system = crate::neo_system::NeoSystem::new(protocol, None, None).expect("neo system");
    let service = OracleService::new(
        OracleServiceSettings {
            network,
            ..Default::default()
        },
        system,
    )
    .expect("oracle service");
    let key_pair = KeyPair::from_private_key(&[0x43; 32]).expect("oracle key");
    let account = key_pair.get_script_hash();
    let registered = KeyRef::new("oracle-hsm", "oracle-key-v6", 6);
    let unknown = KeyRef::new("missing-oracle-provider", "oracle-key-v6", 6);
    let default_fallback = KeyRef::new("software-default", "default-key", 1);
    let resolver = Arc::new(
        SigningKeyResolver::new([SigningKeyBinding {
            account: account.to_string(),
            purpose: SignPurpose::OracleResponse,
            key_ref: unknown.clone(),
            public_key: key_pair.public_key(),
        }])
        .expect("unknown-provider oracle key resolver"),
    );
    let recording = Arc::new(RecordingOracleSigner {
        registered_key_ref: registered.clone(),
        requests: Mutex::new(Vec::new()),
        resume_calls: AtomicUsize::new(0),
    });
    let signer = TrustedSignerClient::new(
        recording.clone(),
        "software-default",
        "oracle-policy-v5",
        Duration::from_secs(5),
        AuthenticatedContext {
            schema_version: SIGNING_SCHEMA_VERSION,
            principal: "oracle-test".to_string(),
            tenant: "node-test".to_string(),
            authentication_method: "local-test".to_string(),
            scopes: BTreeSet::from(["sign:oracle-response".to_string()]),
            claims: BTreeMap::new(),
        },
        resolver,
    )
    .expect("trusted oracle signer");
    service.set_signer_client(signer);

    let error = service
        .sign_neo_payload(
            "oracle-operation-unknown-provider".to_string(),
            account,
            vec![1, 3, 5, 7],
            "neo-n3-oracle-response-submission",
            BTreeMap::from([("request_id".to_string(), "9".to_string())]),
        )
        .await
        .expect_err("unknown provider must fail closed");
    assert!(error.to_string().contains("not registered"));
    let requests = recording.requests.lock().expect("oracle recording lock");
    assert_eq!(requests.len(), 1);
    let (request, context) = &requests[0];
    assert_eq!(request.key_ref, unknown);
    assert_eq!(request.key_ref.provider, "missing-oracle-provider");
    assert_ne!(request.key_ref, registered);
    assert_ne!(request.key_ref, default_fallback);
    assert_eq!(request.purpose, SignPurpose::OracleResponse);
    assert_eq!(request.policy_version, "oracle-policy-v5");
    assert_eq!(
        context.scopes,
        BTreeSet::from(["sign:oracle-response".to_string()])
    );
    assert_eq!(recording.resume_calls.load(Ordering::SeqCst), 1);
}
