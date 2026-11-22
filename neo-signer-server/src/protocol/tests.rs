use super::*;
use crate::config::{ClientAuthorizationPolicy, ClientGrant, FaultPolicy, TlsPaths, key_selector};
use neo_wallets::signing::{DigestAlgorithm, SignPayload, SignPurpose};

fn service_dir(tag: &str) -> (tempfile::TempDir, SignerServerConfig) {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = SignerServerConfig {
        bind_addr: "127.0.0.1:0".parse().expect("bind address"),
        tls: TlsPaths {
            server_certificate_path: dir.path().join("server.pem"),
            server_private_key_path: dir.path().join("server.key"),
            ca_certificate_path: dir.path().join("ca.pem"),
        },
        sealed_data_dir: dir.path().join("sealed-data"),
        sealed_key_path: dir.path().join("sealed-key.json"),
        service_identity: "signer.prod".to_string(),
        policy_version: "policy-v1".to_string(),
        key_ref: KeyRef::new("remote", "validator-1", 2),
        allowed_purposes: vec![SignPurpose::Consensus],
        session_lifetime_ms: 300_000,
        max_challenge_age_ms: 60_000,
        faults: FaultPolicy::default(),
    };
    let _ = tag;
    (dir, config)
}

fn request(request_id: &str, purpose: SignPurpose) -> RemoteSignRequest {
    RemoteSignRequest {
        schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
        request_id: request_id.to_string(),
        key_ref: KeyRef::new("remote", "validator-1", 2),
        network: 860_833_102,
        purpose,
        payload: SignPayload::Digest {
            algorithm: DigestAlgorithm::Sha256,
            digest: [7; 32],
        },
        request_digest: [9; 32],
        policy_version: "policy-v1".to_string(),
        attestation_session_id: None,
    }
}

fn identity() -> ClientIdentity {
    ClientIdentity::from_common_name("neo-node-test-client")
}

fn challenge(nonce: u8) -> AttestationChallenge {
    AttestationChallenge {
        schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
        challenge: [nonce; 32],
        issued_at_unix_ms: now_unix_ms(),
        key_ref: KeyRef::new("remote", "validator-1", 2),
        service_identity: "signer.prod".to_string(),
        policy_version: "policy-v1".to_string(),
    }
}

async fn attest_session(service: &SignerService, identity: &ClientIdentity, nonce: u8) -> String {
    service
        .attest(identity, &challenge(nonce))
        .await
        .expect("attested session")
        .session_id
}

fn with_session(mut request: RemoteSignRequest, session_id: &str) -> RemoteSignRequest {
    request.attestation_session_id = Some(session_id.to_string());
    request
}

fn query_for(request: &RemoteSignRequest) -> RemoteOperationQuery {
    RemoteOperationQuery {
        schema_version: REMOTE_SIGNER_PROTOCOL_VERSION,
        request_id: request.request_id.clone(),
        request_digest: request.request_digest,
        key_ref: request.key_ref.clone(),
        attestation_session_id: request.attestation_session_id.clone(),
    }
}

#[tokio::test]
async fn sign_without_session_is_never_terminal() {
    let (_dir, config) = service_dir("no-session");
    let service = SignerService::new(config).expect("service");
    // No session established: the only safe answer is ambiguity, because
    // the node client cannot authenticate a terminal answer without the
    // attested session key.
    match service.sign(&identity(), &request("req-1", SignPurpose::Consensus)) {
        SignOutcome::Ambiguous(reason) => {
            assert!(reason.contains("session"), "unexpected reason: {reason}");
        }
        _ => panic!("dispatch without a session must stay ambiguous"),
    }
}

#[tokio::test]
async fn key_info_rejects_foreign_keys() {
    let (_dir, config) = service_dir("key-info");
    let service = SignerService::new(config).expect("service");
    assert!(
        service
            .key_info(&identity(), &KeyRef::new("remote", "other-key", 1))
            .is_err()
    );
    let info = service
        .key_info(&identity(), &KeyRef::new("remote", "validator-1", 2))
        .expect("key info");
    assert_eq!(info.key_info.public_key.len(), 33);
    assert!(info.key_info.enabled);
    assert_eq!(info.key_info.algorithm, ALGORITHM_ECDSA_P256_SHA256);
}

#[tokio::test]
async fn attest_rejects_replayed_challenges() {
    let (_dir, config) = service_dir("replay");
    let service = SignerService::new(config).expect("service");
    let challenge = challenge(0x55);
    assert!(service.attest(&identity(), &challenge).await.is_ok());
    let replayed = service
        .attest(&identity(), &challenge)
        .await
        .expect_err("replay");
    assert!(replayed.to_string().contains("replay"));
}

#[tokio::test]
async fn sealed_key_is_reused_across_service_instances() {
    let (dir, config) = service_dir("restart");
    let first = SignerService::new(config.clone()).expect("first boot");
    let second = SignerService::new(config).expect("second boot");
    assert_eq!(first.public_key(), second.public_key());
    let _ = dir; // keeps the tempdir alive
}

#[tokio::test]
async fn completed_operations_survive_a_restart_and_are_never_resigned() {
    let (_dir, config) = service_dir("journal-restart");
    let identity = identity();
    let first = SignerService::new(config.clone()).expect("first boot");
    let session = attest_session(&first, &identity, 1).await;
    let request = with_session(request("req-journal", SignPurpose::Consensus), &session);
    let SignOutcome::Succeeded(response) = first.sign(&identity, &request) else {
        panic!("the first dispatch must sign")
    };
    assert_eq!(first.signatures_issued(), 1);
    assert!(
        first.journal_path().exists(),
        "the journal must live on disk"
    );
    drop(first);

    // Restart on the same sealed-data directory: the journal is replayed
    // from disk, so the operation is recoverable and is never re-signed.
    let second = SignerService::new(config).expect("second boot");
    assert_eq!(
        second.journaled_operations(),
        1,
        "the operation journal must survive a restart"
    );
    match second.sign(&identity, &request) {
        SignOutcome::Succeeded(replayed) => assert_eq!(
            replayed.provider_signature.signature,
            response.provider_signature.signature
        ),
        _ => panic!("a journaled operation must be replayed verbatim"),
    }
    match second
        .query(&identity, &query_for(&request))
        .expect("query")
    {
        RemoteOperationStatus::Succeeded { response: replayed } => assert_eq!(
            replayed.provider_signature.signature,
            response.provider_signature.signature
        ),
        other => panic!("expected the durable result, got {other:?}"),
    }
    assert_eq!(
        second.signatures_issued(),
        0,
        "a restart must never produce a second signature"
    );
}

#[tokio::test]
async fn a_conflicting_consensus_slot_is_refused_durably() {
    let (_dir, config) = service_dir("slot-conflict");
    let identity = identity();
    let slot_payload = |digest: u8| SignPayload::Canonical {
        encoding: "neo-consensus/v1/commit".to_string(),
        bytes: vec![0, 0, 0, 42, 1, digest],
    };

    let first = SignerService::new(config.clone()).expect("first boot");
    let session = attest_session(&first, &identity, 2).await;

    let mut original = with_session(request("req-slot-a", SignPurpose::Consensus), &session);
    original.payload = slot_payload(0x11);
    original.request_digest = [0x11; 32];
    assert!(matches!(
        first.sign(&identity, &original),
        SignOutcome::Succeeded(_)
    ));

    // Same (network, height, view, phase) with a different payload is an
    // equivocation and must never reach the sealed key.
    let mut conflicting = with_session(request("req-slot-b", SignPurpose::Consensus), &session);
    conflicting.payload = slot_payload(0x22);
    conflicting.request_digest = [0x22; 32];
    match first.sign(&identity, &conflicting) {
        SignOutcome::Rejected(rejection) => {
            assert_eq!(rejection.code, "policy_permanent_denial");
            assert!(rejection.permanent);
            assert!(
                rejection.message.contains("consensus slot"),
                "unexpected message: {}",
                rejection.message
            );
        }
        _ => panic!("a conflicting consensus payload must be rejected"),
    }
    assert_eq!(first.signatures_issued(), 1);
    drop(first);

    // The slot reservation is durable: a restarted signer still refuses a
    // new payload for the same slot.
    let second = SignerService::new(config).expect("second boot");
    let session = attest_session(&second, &identity, 3).await;
    let mut late = with_session(request("req-slot-c", SignPurpose::Consensus), &session);
    late.payload = slot_payload(0x33);
    late.request_digest = [0x33; 32];
    match second.sign(&identity, &late) {
        SignOutcome::Rejected(rejection) => {
            assert_eq!(rejection.code, "policy_permanent_denial");
            assert!(rejection.message.contains("consensus slot"));
        }
        _ => panic!("the consensus slot reservation must survive a restart"),
    }
    assert_eq!(second.signatures_issued(), 0);
}

#[tokio::test]
async fn an_unlisted_client_identity_cannot_reach_the_signing_path() {
    let (_dir, config) = service_dir("identity");
    let key = config.key_ref.clone();
    let policy = ClientAuthorizationPolicy {
        grants: vec![ClientGrant::new(
            "neo-node-1",
            vec![key_selector(&key)],
            vec![SignPurpose::Consensus],
        )],
    };
    let service = SignerService::with_client_authorization(config, policy).expect("service");
    let allowed = ClientIdentity::from_common_name("neo-node-1");
    let stranger = ClientIdentity::from_common_name("neo-node-2");

    // Attestation, key metadata and signing are all scoped by identity.
    assert!(service.attest(&stranger, &challenge(4)).await.is_err());
    assert!(service.key_info(&stranger, &key).is_err());
    assert_eq!(service.attestation_sessions_issued(), 0);

    let session = attest_session(&service, &allowed, 5).await;
    let allowed_request = with_session(request("req-id", SignPurpose::Consensus), &session);
    assert!(matches!(
        service.sign(&allowed, &allowed_request),
        SignOutcome::Succeeded(_)
    ));

    // The stranger is refused before the journal and the sealed key.
    assert!(matches!(
        service.sign(&stranger, &allowed_request),
        SignOutcome::Unauthorized(_)
    ));
    assert!(
        service
            .query(&stranger, &query_for(&allowed_request))
            .is_err()
    );
    assert_eq!(service.signatures_issued(), 1);
}

#[tokio::test]
async fn sessions_are_bound_to_the_identity_that_attested() {
    let (_dir, config) = service_dir("identity-binding");
    let key = config.key_ref.clone();
    let policy = ClientAuthorizationPolicy {
        grants: vec![
            ClientGrant::new(
                "neo-node-1",
                vec![key_selector(&key)],
                vec![SignPurpose::Consensus],
            ),
            ClientGrant::new(
                "neo-node-2",
                vec![key_selector(&key)],
                vec![SignPurpose::Consensus],
            ),
        ],
    };
    let service = SignerService::with_client_authorization(config, policy).expect("service");
    let node1 = ClientIdentity::from_common_name("neo-node-1");
    let node2 = ClientIdentity::from_common_name("neo-node-2");
    let session = attest_session(&service, &node1, 6).await;
    let stolen = with_session(request("req-steal", SignPurpose::Consensus), &session);
    match service.sign(&node2, &stolen) {
        SignOutcome::Ambiguous(reason) => {
            assert!(reason.contains("session"), "unexpected reason: {reason}");
        }
        _ => panic!("another identity must not be able to use the session"),
    }
    assert_eq!(service.signatures_issued(), 0);
}
