//! Reference binary for the mTLS remote TEE signer service.
//!
//! **Reference implementation.** The attestation evidence is produced by the
//! neo-tee simulation enclave; it does not satisfy a production
//! `RemoteHardwareTee` policy. See the crate documentation of
//! `neo_signer_server` for the production deployment requirements.

use anyhow::{Context, Result, bail};
use clap::Parser;
use neo_signer_server::config::{DEFAULT_SESSION_LIFETIME_MS, MAX_ATTESTATION_CLOCK_SKEW_MS};
use neo_signer_server::{FaultPolicy, SignerServerConfig, TlsPaths, spawn_signer_server};
use neo_wallets::signing::{KeyRef, SignPurpose};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "neo-signer-server")]
#[command(about = "Reference mTLS remote TEE signer service (simulation attestation)")]
struct Args {
    /// HTTPS listen address.
    #[arg(long, default_value = "127.0.0.1:4261")]
    bind: SocketAddr,
    /// PEM file with the server certificate.
    #[arg(long)]
    tls_certificate: PathBuf,
    /// PEM file with the server private key.
    #[arg(long)]
    tls_private_key: PathBuf,
    /// PEM file with the CA certificate that client certificates must chain to.
    #[arg(long)]
    ca_certificate: PathBuf,
    /// neo-tee sealed-data directory (monotonic counter + rollback floor).
    #[arg(long)]
    sealed_data_dir: PathBuf,
    /// File the sealed signing key is persisted to.
    #[arg(long)]
    sealed_key_file: PathBuf,
    /// Attested service identity.
    #[arg(long, default_value = "signer.prod")]
    service_identity: String,
    /// Signing policy version.
    #[arg(long, default_value = "policy-v1")]
    policy_version: String,
    /// Key provider namespace.
    #[arg(long, default_value = "remote")]
    provider: String,
    /// Key identifier.
    #[arg(long, default_value = "validator-1")]
    key_id: String,
    /// Immutable key version.
    #[arg(long, default_value_t = 1)]
    key_version: u64,
    /// Purposes this signer authorizes (repeatable). Only these purposes are
    /// authorized; anything else receives an authenticated permanent
    /// rejection.
    #[arg(
        long = "allow-purpose",
        value_delimiter = ',',
        default_value = "consensus"
    )]
    allowed_purposes: Vec<String>,

    // === Fault injection (test/staging only; never enable in production) ===
    /// [TEST ONLY] Skew the attestation evidence timestamp by this many
    /// milliseconds.
    #[arg(long, default_value_t = 0)]
    attest_clock_skew_ms: i64,
    /// [TEST ONLY] Delay the attest response by this many milliseconds.
    #[arg(long, default_value_t = 0)]
    attest_delay_ms: u64,
    /// [TEST ONLY] Answer the first sign dispatch ambiguously (HTTP 503) and
    /// complete it on the first query.
    #[arg(long)]
    hold_dispatch: bool,
}

fn parse_purpose(value: &str) -> Result<SignPurpose> {
    match value.trim().to_ascii_lowercase().as_str() {
        "consensus" => Ok(SignPurpose::Consensus),
        "transaction" => Ok(SignPurpose::Transaction),
        "oracle-response" | "oracle" => Ok(SignPurpose::OracleResponse),
        "state-root" => Ok(SignPurpose::StateRoot),
        "cross-chain" => Ok(SignPurpose::CrossChain),
        other => bail!("unsupported purpose '{other}'"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let mut allowed_purposes = Vec::new();
    for value in &args.allowed_purposes {
        allowed_purposes.push(
            parse_purpose(value)
                .with_context(|| format!("failed to parse allowed purpose '{value}'"))?,
        );
    }

    let faults = FaultPolicy {
        attest_clock_skew_ms: args.attest_clock_skew_ms,
        attest_delay_ms: args.attest_delay_ms,
        hold_dispatch: args.hold_dispatch,
    };
    if (args.attest_clock_skew_ms != 0 || args.attest_delay_ms != 0 || args.hold_dispatch)
        && std::env::var_os("NEO_SIGNER_SERVER_ALLOW_FAULTS").is_none()
    {
        bail!(
            "fault-injection flags require NEO_SIGNER_SERVER_ALLOW_FAULTS=1; \
             they are never valid for production deployments"
        );
    }

    let config = SignerServerConfig {
        bind_addr: args.bind,
        tls: TlsPaths {
            server_certificate_path: args.tls_certificate,
            server_private_key_path: args.tls_private_key,
            ca_certificate_path: args.ca_certificate,
        },
        sealed_data_dir: args.sealed_data_dir,
        sealed_key_path: args.sealed_key_file,
        service_identity: args.service_identity,
        policy_version: args.policy_version,
        key_ref: KeyRef::new(args.provider, args.key_id, args.key_version),
        allowed_purposes,
        session_lifetime_ms: DEFAULT_SESSION_LIFETIME_MS,
        max_challenge_age_ms: MAX_ATTESTATION_CLOCK_SKEW_MS,
        faults,
    };

    let server = spawn_signer_server(config).await?;
    let addr = server.local_addr();
    tracing::info!(
        "neo-signer-server listening on https://{addr} (simulation attestation; reference implementation)"
    );

    tokio::signal::ctrl_c()
        .await
        .context("failed to wait for ctrl-c")?;
    server.shutdown().await;
    Ok(())
}
