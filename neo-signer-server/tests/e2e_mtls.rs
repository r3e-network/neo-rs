//! End-to-end mTLS tests for the reference signer server.
//!
//! Each scenario drives a real HTTPS server over TLS with a client that ports
//! the node-side `RemoteSignerProvider` validation (see [`common::client`])
//! against the exact neo-core wire types:
//!
//! * [`full_success_path_signs_consensus_payload`] — challenge → evidence →
//!   Consensus signing → independently verified signature;
//! * [`result_unknown_recovers_through_query_without_double_signing`] —
//!   ambiguous dispatch (HTTP 5xx) recovers via the idempotent query endpoint;
//! * [`unauthorized_purpose_is_an_authenticated_permanent_rejection`] —
//!   terminal `Failed` with a session-authenticated, request-bound rejection;
//! * [`skewed_evidence_timestamp_is_rejected_by_the_client`] — clock-skew
//!   fault injection beyond `MAX_ATTESTATION_CLOCK_SKEW_MS`;
//! * [`replayed_attestation_challenge_is_refused`] — single-use nonces;
//! * [`client_without_certificate_is_refused`] — mTLS enforcement plus a
//!   plaintext request against the TLS-only listener;
//! * [`sealed_signing_key_survives_a_server_restart`] — SealedKey-backed key
//!   persists across restarts and keeps signing under the same public key.

mod common;

use common::client::MtlsSignerClient;
use common::{client_policy, start_signer, start_signer_with};
use neo_core::signing::{
    REMOTE_SIGNER_PROTOCOL_VERSION, RemoteOperationStatus, RemoteSignDispatchResult, SignErrorCode,
    SignPurpose,
};
use neo_crypto::Secp256r1Crypto;
use neo_signer_server::FaultPolicy;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const NETWORK: u32 = 860_833_102;

/// Extracts a 64-byte provider signature from a sign response.
fn signature_bytes(response: &neo_core::signing::RemoteSignResponse) -> [u8; 64] {
    response
        .provider_signature
        .signature
        .as_slice()
        .try_into()
        .expect("provider signature must be 64 bytes")
}

/// Verifies a consensus digest signature against the server's sealed key.
fn verify_provider_signature(
    response: &neo_core::signing::RemoteSignResponse,
    digest: [u8; 32],
    public_key: &[u8],
) -> bool {
    Secp256r1Crypto::verify_prehash(&digest, &signature_bytes(response), public_key)
        .unwrap_or(false)
}

#[tokio::test]
async fn full_success_path_signs_consensus_payload() {
    let harness = start_signer(FaultPolicy::default()).await;
    let client = MtlsSignerClient::new(
        harness.endpoint.clone(),
        &harness.pki,
        client_policy(&harness),
    );

    // 1. Key metadata for the configured immutable key.
    let key_info = client.key_info().await.expect("key-info must succeed");
    let sealed_public_key = harness.server.service().public_key();
    assert_eq!(key_info.key_info.public_key, sealed_public_key);
    assert_eq!(
        key_info.key_info.schema_version,
        REMOTE_SIGNER_PROTOCOL_VERSION
    );

    // 2. Attestation: challenge → evidence passes the full client-side
    //    freshness, binding, measurement and verified-claims validation.
    let session = client.ensure_session().await.expect("attested session");
    assert_eq!(harness.server.service().attestation_sessions_issued(), 1);

    // 3. Consensus-purpose dispatch.
    let digest = [0x42; 32];
    let request = client.build_sign_request(
        &session,
        "req-e2e-success",
        SignPurpose::Consensus,
        digest,
        NETWORK,
    );
    let response = client
        .dispatch_and_validate(&request, &session)
        .await
        .expect("consensus signature");
    assert!(verify_provider_signature(
        &response,
        digest,
        &sealed_public_key
    ));

    // 4. Idempotency: re-dispatching the same request replays the journaled
    //    response and never re-signs.
    let replayed = client
        .dispatch_and_validate(&request, &session)
        .await
        .expect("replayed response");
    assert_eq!(
        replayed.provider_signature.signature,
        response.provider_signature.signature
    );
    assert_eq!(harness.server.service().signatures_issued(), 1);

    // 5. The query endpoint serves the same durable result.
    let status = client
        .query_unknown(Some(&session), &request.request_id, request.request_digest)
        .await
        .expect("query");
    match status {
        RemoteOperationStatus::Succeeded { response } => {
            assert_eq!(
                response.provider_signature.signature,
                replayed.provider_signature.signature
            );
        }
        other => panic!("expected Succeeded, got {other:?}"),
    }
    assert_eq!(harness.server.service().signatures_issued(), 1);

    harness.server.shutdown().await;
}

#[tokio::test]
async fn result_unknown_recovers_through_query_without_double_signing() {
    let harness = start_signer(FaultPolicy {
        hold_dispatch: true,
        ..FaultPolicy::default()
    })
    .await;
    let client = MtlsSignerClient::new(
        harness.endpoint.clone(),
        &harness.pki,
        client_policy(&harness),
    );
    let session = client.ensure_session().await.expect("attested session");

    let digest = [0x77; 32];
    let request = client.build_sign_request(
        &session,
        "req-e2e-recovery",
        SignPurpose::Consensus,
        digest,
        NETWORK,
    );

    // 1. The dispatch fails ambiguously (HTTP 5xx after dispatch): the client
    //    must classify it as retryable ResultUnknown, never as terminal.
    let error = client
        .dispatch_and_validate(&request, &session)
        .await
        .expect_err("dispatch must be ambiguous");
    assert_eq!(error.code, SignErrorCode::ResultUnknown);
    assert!(error.retryable, "ResultUnknown must stay retryable");

    // 2. First query: the server completes the deferred signing but reports
    //    Pending for the query that arrived before completion.
    let first = client
        .query_unknown(Some(&session), &request.request_id, request.request_digest)
        .await
        .expect("first query");
    assert!(
        matches!(first, RemoteOperationStatus::Pending),
        "expected Pending, got {first:?}"
    );

    // 3. The deferred completion signed exactly once.
    assert_eq!(harness.server.service().signatures_issued(), 1);

    // 4. Second query: the durable result, validated by the client.
    let second = client
        .query_unknown(Some(&session), &request.request_id, request.request_digest)
        .await
        .expect("second query");
    let RemoteOperationStatus::Succeeded { response } = second else {
        panic!("expected Succeeded, got {second:?}")
    };
    assert!(verify_provider_signature(
        &response,
        digest,
        &harness.server.service().public_key()
    ));

    // 5. A late re-dispatch replays the journaled result instead of signing.
    let replayed = client
        .dispatch_and_validate(&request, &session)
        .await
        .expect("replayed response after recovery");
    assert_eq!(
        replayed.provider_signature.signature,
        response.provider_signature.signature
    );
    assert_eq!(harness.server.service().signatures_issued(), 1);

    harness.server.shutdown().await;
}

#[tokio::test]
async fn unauthorized_purpose_is_an_authenticated_permanent_rejection() {
    let harness = start_signer(FaultPolicy::default()).await;
    let client = MtlsSignerClient::new(
        harness.endpoint.clone(),
        &harness.pki,
        client_policy(&harness),
    );
    let session = client.ensure_session().await.expect("attested session");

    // Transaction is not an authorized purpose for this key.
    let request = client.build_sign_request(
        &session,
        "req-e2e-denied",
        SignPurpose::Transaction,
        [0x11; 32],
        NETWORK,
    );

    // 1. The wire outcome is a 4xx whose body is a RemoteSignRejection that
    //    the node client can authenticate and terminalize.
    let rejection = match client
        .dispatch_raw(&request)
        .await
        .expect("dispatch outcome")
    {
        RemoteSignDispatchResult::Rejected(rejection) => rejection,
        other => panic!("expected a permanent rejection, got {other:?}"),
    };
    assert_eq!(rejection.code, "authorization_denied");
    assert!(rejection.permanent);
    client
        .validate_rejection(&request, &rejection, Some(&session))
        .expect("rejection must satisfy the node-side validation");

    // 2. The rejection is journaled: re-dispatch replays it verbatim.
    let replayed = match client
        .dispatch_raw(&request)
        .await
        .expect("replayed dispatch outcome")
    {
        RemoteSignDispatchResult::Rejected(rejection) => rejection,
        other => panic!("expected the journaled rejection, got {other:?}"),
    };
    assert_eq!(replayed.code, rejection.code);
    assert_eq!(replayed.session_signature, rejection.session_signature);

    // 3. The query endpoint reports a terminal Failed with the same
    //    authenticated rejection.
    let status = client
        .query_unknown(Some(&session), &request.request_id, request.request_digest)
        .await
        .expect("query");
    match status {
        RemoteOperationStatus::Failed { rejection } => {
            client
                .validate_rejection(&request, &rejection, Some(&session))
                .expect("journaled rejection must stay authenticated");
            assert_eq!(rejection.code, "authorization_denied");
        }
        other => panic!("expected Failed, got {other:?}"),
    }

    // 4. A denied purpose never touches the sealed key.
    assert_eq!(harness.server.service().signatures_issued(), 0);

    harness.server.shutdown().await;
}

#[tokio::test]
async fn skewed_evidence_timestamp_is_rejected_by_the_client() {
    // Fault injection: the server stamps evidence 90s into the future, beyond
    // the node's MAX_ATTESTATION_CLOCK_SKEW_MS = 60_000 window.
    let harness = start_signer(FaultPolicy {
        attest_clock_skew_ms: 90_000,
        ..FaultPolicy::default()
    })
    .await;
    let client = MtlsSignerClient::new(
        harness.endpoint.clone(),
        &harness.pki,
        client_policy(&harness),
    );

    let error = client
        .ensure_session()
        .await
        .expect_err("skewed evidence must fail client validation");
    assert_eq!(
        error.code,
        SignErrorCode::VerificationFailed,
        "expected a verification failure, got: {error}"
    );

    harness.server.shutdown().await;
}

#[tokio::test]
async fn replayed_attestation_challenge_is_refused() {
    let harness = start_signer(FaultPolicy::default()).await;
    let client = MtlsSignerClient::new(
        harness.endpoint.clone(),
        &harness.pki,
        client_policy(&harness),
    );

    // A fresh challenge yields evidence that passes the full validation.
    let session = client
        .ensure_session()
        .await
        .expect("fresh challenge works");
    assert_eq!(harness.server.service().attestation_sessions_issued(), 1);

    // The same nonce cannot be used twice: the server refuses the replay
    // with an HTTP 4xx on the attest endpoint.
    let challenge = client.make_challenge();
    client
        .attest_raw(&challenge)
        .await
        .expect("first use of the nonce");
    let replay = client
        .attest_raw(&challenge)
        .await
        .expect_err("nonce replay must be refused");
    assert!(
        replay.to_string().contains("HTTP 4"),
        "expected an HTTP 4xx refusal, got: {replay}"
    );

    // No session was created for the refused replay.
    assert_eq!(harness.server.service().attestation_sessions_issued(), 2);
    let _ = session;

    harness.server.shutdown().await;
}

#[tokio::test]
async fn client_without_certificate_is_refused() {
    let harness = start_signer(FaultPolicy::default()).await;
    let anonymous = MtlsSignerClient::anonymous(
        harness.endpoint.clone(),
        &harness.pki,
        client_policy(&harness),
    );

    // Without a client certificate every request dies in the TLS handshake.
    let key_info_error = anonymous.key_info().await.expect_err("anonymous key-info");
    assert!(
        key_info_error.to_string().contains("transport failed"),
        "expected a transport failure, got: {key_info_error}"
    );
    let challenge = anonymous.make_challenge();
    let attest_error = anonymous
        .attest_raw(&challenge)
        .await
        .expect_err("anonymous attest");
    assert!(
        attest_error.to_string().contains("transport failed"),
        "expected a transport failure, got: {attest_error}"
    );
    assert_eq!(harness.server.service().attestation_sessions_issued(), 0);

    // The listener is TLS-only: a plaintext HTTP request never gets an HTTP
    // answer. (rustls may emit a short TLS alert record before closing; that
    // is not an HTTP response, so only a response starting with "HTTP/"
    // counts as a protocol violation.)
    let mut plain = tokio::net::TcpStream::connect(harness.server.local_addr())
        .await
        .expect("TCP connect");
    plain
        .write_all(b"POST /v1/signer/attest HTTP/1.1\r\nHost: signer\r\n\r\n")
        .await
        .expect("plaintext write");
    let mut received = Vec::new();
    let mut buffer = [0u8; 256];
    loop {
        match plain.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => received.extend_from_slice(&buffer[..read]),
        }
    }
    assert!(
        !received.starts_with(b"HTTP/"),
        "plaintext must never receive an HTTP response, got {} bytes: {:?}",
        received.len(),
        String::from_utf8_lossy(&received)
    );

    harness.server.shutdown().await;
}

#[tokio::test]
async fn sealed_signing_key_survives_a_server_restart() {
    let first = start_signer(FaultPolicy::default()).await;
    let first_public_key = first.server.service().public_key();

    // Sign once on the first boot.
    {
        let client =
            MtlsSignerClient::new(first.endpoint.clone(), &first.pki, client_policy(&first));
        let session = client.ensure_session().await.expect("attested session");
        let request = client.build_sign_request(
            &session,
            "req-e2e-restart",
            SignPurpose::Consensus,
            [0x66; 32],
            NETWORK,
        );
        let response = client
            .dispatch_and_validate(&request, &session)
            .await
            .expect("signature on first boot");
        assert!(verify_provider_signature(
            &response,
            [0x66; 32],
            &first_public_key
        ));
    }
    assert_eq!(first.server.service().signatures_issued(), 1);
    first.server.shutdown().await;

    // Restart on the same sealed-data directory and sealed-key file: the
    // SealedKey must unseal to the same signing key (rollback floor intact).
    let tls_files = tempfile::TempDir::new().expect("restart TLS files");
    let second = start_signer_with(
        FaultPolicy::default(),
        tls_files,
        first.sealed_data_dir.clone(),
        first.sealed_key_path.clone(),
    )
    .await;
    assert_eq!(
        second.server.service().public_key(),
        first_public_key,
        "the sealed signing key must survive the restart"
    );

    // The restarted server signs under the same public key.
    let client =
        MtlsSignerClient::new(second.endpoint.clone(), &second.pki, client_policy(&second));
    let session = client.ensure_session().await.expect("attested session");
    let request = client.build_sign_request(
        &session,
        "req-e2e-restart-2",
        SignPurpose::Consensus,
        [0x67; 32],
        NETWORK,
    );
    let response = client
        .dispatch_and_validate(&request, &session)
        .await
        .expect("signature on second boot");
    assert!(verify_provider_signature(
        &response,
        [0x67; 32],
        &first_public_key
    ));
    assert_eq!(second.server.service().signatures_issued(), 1);

    second.server.shutdown().await;
}
