//! Shared fixtures for the neo-signer-server end-to-end mTLS tests.
//!
//! The node's `MtlsRemoteSignerTransport` and `RemoteSignerProvider` are
//! `pub(crate)` inside neo-node and cannot be imported from this crate, so the
//! harness re-implements the client-side contract (see [`client`]) on top of
//! the exact neo-core wire types and the same reqwest/rustls stack the node
//! transport uses.

#![allow(dead_code)]

pub mod client;

use neo_signer_server::{
    FaultPolicy, SignerServerConfig, TlsPaths,
    config::{DEFAULT_SESSION_LIFETIME_MS, MAX_ATTESTATION_CLOCK_SKEW_MS},
    spawn_signer_server,
};
use crate::signing::KeyRef;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Self-signed PKI for one test run: one CA, one server certificate, one
/// node-client certificate.
pub struct Pki {
    pub ca_cert_pem: Vec<u8>,
    pub server_cert_pem: Vec<u8>,
    pub server_key_pem: Vec<u8>,
    pub client_cert_pem: Vec<u8>,
    pub client_key_pem: Vec<u8>,
}

/// Generates a fresh CA + server (SAN: 127.0.0.1, localhost) + client
/// certificate chain with rcgen.
pub fn generate_pki() -> Pki {
    use rcgen::{
        BasicConstraints, Certificate, CertificateParams, DnType, IsCa, KeyPair,
        PKCS_ECDSA_P256_SHA256, SanType,
    };
    use std::net::{IpAddr, Ipv4Addr};

    // CA (self-signed). The signing key is bound through `key_pair` so the
    // certificate can sign the server/client leaves.
    let ca_key = KeyPair::generate(&PKCS_ECDSA_P256_SHA256).expect("CA key");
    let mut ca_params = CertificateParams::new(vec![]);
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Neo Signer Test Root CA");
    ca_params.key_pair = Some(ca_key);
    let ca_cert = Certificate::from_params(ca_params).expect("self-signed CA certificate");

    // Server certificate (SAN: 127.0.0.1, localhost) signed by the CA.
    let server_key = KeyPair::generate(&PKCS_ECDSA_P256_SHA256).expect("server key");
    let server_key_pem = server_key.serialize_pem();
    let mut server_params = CertificateParams::new(vec![]);
    server_params
        .distinguished_name
        .push(DnType::CommonName, "neo-signer-server");
    server_params.subject_alt_names = vec![
        SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        SanType::DnsName("localhost".to_string()),
    ];
    server_params.key_pair = Some(server_key);
    let server_cert = Certificate::from_params(server_params).expect("server certificate");
    let server_cert_pem = server_cert
        .serialize_pem_with_signer(&ca_cert)
        .expect("server certificate PEM");

    // Node-client certificate signed by the same CA.
    let client_key = KeyPair::generate(&PKCS_ECDSA_P256_SHA256).expect("client key");
    let client_key_pem = client_key.serialize_pem();
    let mut client_params = CertificateParams::new(vec![]);
    client_params
        .distinguished_name
        .push(DnType::CommonName, "neo-node-test-client");
    client_params.key_pair = Some(client_key);
    let client_cert = Certificate::from_params(client_params).expect("client certificate");
    let client_cert_pem = client_cert
        .serialize_pem_with_signer(&ca_cert)
        .expect("client certificate PEM");

    Pki {
        ca_cert_pem: ca_cert
            .serialize_pem()
            .expect("CA certificate PEM")
            .into_bytes(),
        server_cert_pem: server_cert_pem.into_bytes(),
        server_key_pem: server_key_pem.into_bytes(),
        client_cert_pem: client_cert_pem.into_bytes(),
        client_key_pem: client_key_pem.into_bytes(),
    }
}

/// A running signer server plus everything a test client needs to reach it.
pub struct Harness {
    pub server: neo_signer_server::SignerServerHandle,
    pub endpoint: reqwest::Url,
    pub pki: Pki,
    pub service_identity: String,
    pub policy_version: String,
    pub key_ref: KeyRef,
    pub sealed_data_dir: PathBuf,
    pub sealed_key_path: PathBuf,
    _files: TempDir,
}

/// Locations of the neo-tee sealed state inside a temp directory.
pub fn sealed_paths(files: &TempDir) -> (PathBuf, PathBuf) {
    (
        files.path().join("sealed-data"),
        files.path().join("sealed-key.json"),
    )
}

/// Starts a signer server with a fresh PKI and fresh sealed state, applying
/// the given fault-injection policy.
pub async fn start_signer(faults: FaultPolicy) -> Harness {
    let files = TempDir::new().expect("tempdir for server files");
    let (sealed_data_dir, sealed_key_path) = sealed_paths(&files);
    start_signer_with(faults, files, sealed_data_dir, sealed_key_path).await
}

/// Starts a signer server reusing externally managed sealed-state locations
/// (used by the restart test).
pub async fn start_signer_with(
    faults: FaultPolicy,
    files: TempDir,
    sealed_data_dir: PathBuf,
    sealed_key_path: PathBuf,
) -> Harness {
    let pki = generate_pki();
    write_file(&files.path().join("server.pem"), &pki.server_cert_pem);
    write_file(&files.path().join("server.key"), &pki.server_key_pem);
    write_file(&files.path().join("ca.pem"), &pki.ca_cert_pem);

    let key_ref = KeyRef::new("remote", "validator-1", 2);
    let config = SignerServerConfig {
        bind_addr: "127.0.0.1:0".parse().expect("bind address"),
        tls: TlsPaths {
            server_certificate_path: files.path().join("server.pem"),
            server_private_key_path: files.path().join("server.key"),
            ca_certificate_path: files.path().join("ca.pem"),
        },
        sealed_data_dir: sealed_data_dir.clone(),
        sealed_key_path: sealed_key_path.clone(),
        service_identity: "signer.prod".to_string(),
        policy_version: "policy-v1".to_string(),
        key_ref: key_ref.clone(),
        allowed_purposes: vec![neo_signing::SignPurpose::Consensus],
        session_lifetime_ms: DEFAULT_SESSION_LIFETIME_MS,
        max_challenge_age_ms: MAX_ATTESTATION_CLOCK_SKEW_MS,
        faults,
    };

    let server = spawn_signer_server(config).await.expect("server starts");
    let endpoint = reqwest::Url::parse(&format!(
        "https://127.0.0.1:{}/",
        server.local_addr().port()
    ))
    .expect("endpoint URL");

    Harness {
        server,
        endpoint,
        pki,
        service_identity: "signer.prod".to_string(),
        policy_version: "policy-v1".to_string(),
        key_ref,
        sealed_data_dir,
        sealed_key_path,
        _files: files,
    }
}

/// Default client expectations matching the neo-tee simulation enclave
/// measurements used by the server's attestation reports.
pub fn client_policy(harness: &Harness) -> client::ClientPolicy {
    client::ClientPolicy {
        service_identity: harness.service_identity.clone(),
        policy_version: harness.policy_version.clone(),
        key_ref: harness.key_ref.clone(),
        max_challenge_age_ms: 10_000,
        max_clock_skew_ms: client::MAX_ATTESTATION_CLOCK_SKEW_MS,
        max_session_age_ms: 600_000,
        measurement: neo_crypto::Crypto::sha256(b"neo-tee-enclave-v1").to_vec(),
        signer_measurement: Some(neo_crypto::Crypto::sha256(b"neo-tee-signer-v1").to_vec()),
        product_id: 1,
        minimum_security_version: 1,
    }
}

fn write_file(path: &Path, contents: &[u8]) {
    std::fs::write(path, contents)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
}
