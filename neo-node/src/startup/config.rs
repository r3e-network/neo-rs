//! Startup configuration, storage provider selection, and validation.

use crate::cli::NodeCli;
use crate::config::{NodeConfig, SignerProviderKind, infer_magic_from_type};
use crate::probe_containment::NativeProbeContainment;
use crate::probe_controller::validate_probe_startup;
use crate::remote_signer::{MAX_ATTESTATION_CLOCK_SKEW_MS, validate_remote_provider_config};
use crate::signing_provider::NEO_SIGN_DATA_ENCODING;
use anyhow::{Context, Result, bail};
#[cfg(feature = "full")]
use neo_core::persistence::providers::{RocksDBStoreProvider, rocksdb::BatchCommitConfig};
use neo_core::{
    UnhandledExceptionPolicy,
    cryptography::{Crypto, ECCurve, ECPoint},
    persistence::{StoreProvider, storage::StorageConfig},
    protocol_settings::ProtocolSettings,
    smart_contract::helper::Helper as ContractHelper,
    state_service::state_store::StateServiceSettings,
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};
use tracing::{info, warn};

pub(crate) const STORAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) fn select_store_provider(
    backend: Option<&str>,
    storage_config: StorageConfig,
) -> Result<Option<Arc<dyn StoreProvider>>> {
    let Some(name) = backend else {
        return Ok(None);
    };

    let normalized = name.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "memory" | "mem" | "inmemory" => Ok(None),
        "rocksdb" | "rocksdbstore" | "rocksdb-store" => {
            #[cfg(not(feature = "full"))]
            {
                let _ = storage_config;
                bail!(
                    "storage backend '{}' requires neo-node full feature support",
                    name
                );
            }

            #[cfg(feature = "full")]
            {
                let batch_config = rocksdb_batch_config_from_env();
                let provider: Arc<dyn StoreProvider> = Arc::new(
                    RocksDBStoreProvider::new(storage_config).with_batch_config(batch_config),
                );
                Ok(Some(provider))
            }
        }
        other => bail!("unsupported storage backend '{}'", other),
    }
}

#[cfg(feature = "full")]
fn rocksdb_batch_config_from_env() -> BatchCommitConfig {
    let raw = std::env::var("NEO_ROCKSDB_BATCH_PROFILE").unwrap_or_default();
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "balanced" => BatchCommitConfig::balanced(),
        "durable" => {
            info!(target: "neo", profile = "durable", "using RocksDB batch profile");
            BatchCommitConfig::durable()
        }
        "high_throughput" | "high-throughput" | "throughput" => {
            warn!(
                target: "neo",
                profile = "high_throughput",
                "using RocksDB high-throughput batch profile (reduced crash durability)"
            );
            BatchCommitConfig::high_throughput()
        }
        other => {
            warn!(
                target: "neo",
                profile = other,
                "unknown NEO_ROCKSDB_BATCH_PROFILE value; falling back to balanced"
            );
            BatchCommitConfig::balanced()
        }
    }
}

pub(crate) fn check_storage_network(path: &str, magic: u32, read_only: bool) -> Result<()> {
    let storage_path = Path::new(path);
    if !storage_path.exists() {
        if read_only {
            bail!("storage path {} does not exist (read-only mode)", path);
        }
        fs::create_dir_all(storage_path)
            .with_context(|| format!("failed to create storage path {}", path))?;
    }

    let marker = storage_path.join("NETWORK_MAGIC");
    if marker.exists() {
        let contents = fs::read_to_string(&marker)
            .with_context(|| format!("failed to read network marker {}", marker.display()))?;
        let parsed = contents.trim_start_matches("0x").trim().to_string();
        let stored_magic = u32::from_str_radix(&parsed, 16)
            .or_else(|_| parsed.parse::<u32>())
            .with_context(|| format!("invalid magic in {}: {}", marker.display(), contents))?;
        if stored_magic != magic {
            bail!(
                "storage at {} was initialized for network magic 0x{:08x}, but current config is 0x{:08x}. Use a fresh storage path or clear the directory.",
                path,
                stored_magic,
                magic
            );
        }
    } else {
        if read_only {
            bail!(
                "storage path {} missing NETWORK_MAGIC marker (read-only mode)",
                path
            );
        }
        fs::write(&marker, format!("0x{magic:08x}"))
            .with_context(|| format!("failed to write network marker {}", marker.display()))?;
    }

    let version_marker = storage_path.join("VERSION");
    if version_marker.exists() {
        let contents = fs::read_to_string(&version_marker).with_context(|| {
            format!(
                "failed to read storage version marker {}",
                version_marker.display()
            )
        })?;
        let stored_version = contents.trim();
        if stored_version != STORAGE_VERSION {
            bail!(
                "storage at {} was created with version '{}', current binary is '{}'. Use a fresh storage path or migrate data.",
                path,
                stored_version,
                STORAGE_VERSION
            );
        }
    } else {
        if read_only {
            bail!(
                "storage path {} missing VERSION marker (read-only mode)",
                path
            );
        }
        fs::write(&version_marker, STORAGE_VERSION).with_context(|| {
            format!(
                "failed to write storage version marker {}",
                version_marker.display()
            )
        })?;
    }
    Ok(())
}

pub(crate) fn is_public_bind(bind: &str) -> bool {
    bind.parse::<std::net::IpAddr>()
        .map(|ip| !ip.is_loopback())
        .unwrap_or(true)
}

fn has_default_rpc_credentials(user: &str, pass: &str) -> bool {
    if !user.eq_ignore_ascii_case("neo") {
        return false;
    }

    let normalized = pass.trim().to_ascii_lowercase();
    normalized.starts_with("change-me-") || normalized == "change-me" || normalized == "changeme"
}

pub(crate) fn validate_node_config(
    node_config: &NodeConfig,
    storage_path: Option<&str>,
    backend_name: Option<&str>,
    protocol_settings: &ProtocolSettings,
    rpc_hardened: bool,
) -> Result<()> {
    if node_config.rpc.auth_enabled
        && (node_config
            .rpc
            .rpc_user
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
            || node_config
                .rpc
                .rpc_pass
                .as_deref()
                .is_none_or(|value| value.trim().is_empty()))
    {
        bail!("rpc.auth_enabled requires non-blank rpc_user and rpc_pass");
    }

    if node_config.rpc.enabled
        && node_config.rpc.auth_enabled
        && let (Some(user), Some(pass)) = (&node_config.rpc.rpc_user, &node_config.rpc.rpc_pass)
        && has_default_rpc_credentials(user, pass)
    {
        let bind = node_config
            .rpc
            .bind_address
            .as_deref()
            .unwrap_or("127.0.0.1");
        if is_public_bind(bind) {
            bail!(
                "default rpc credentials are not allowed on public bind addresses; set unique rpc_user and rpc_pass"
            );
        }
        warn!(
            target: "neo",
            bind_address = bind,
            "RPC is using template credentials on loopback; change rpc_user/rpc_pass before exposing RPC"
        );
    }

    if node_config
        .blockchain
        .max_free_transactions_per_block
        .is_some()
    {
        bail!(
            "blockchain.max_free_transactions_per_block is not supported by neo-node yet; remove this setting"
        );
    }

    if let Some(limit) = node_config
        .mempool
        .as_ref()
        .and_then(|m| m.max_transactions_per_sender)
        && limit == 0
    {
        bail!("mempool.max_transactions_per_sender must be greater than zero");
    }

    if node_config
        .diagnostics
        .as_ref()
        .is_some_and(|diagnostics| diagnostics.restricted_probe_enabled && !diagnostics.enabled)
    {
        bail!("diagnostics.restricted_probe_enabled requires diagnostics.enabled=true");
    }

    if let Some(diagnostics) = node_config.diagnostics.as_ref()
        && diagnostics.enabled
    {
        if diagnostics.evidence_queue_capacity == 0
            || diagnostics.sampling_interval_ms == 0
            || diagnostics.event_freshness_ms == 0
            || diagnostics.quarantine_max_entries == 0
            || diagnostics.quarantine_max_entries > 1_024
            || diagnostics.proposal_scan_budget != 2_048
            || diagnostics.quarantine_ttl_ms == 0
            || diagnostics.quarantine_ttl_ms > 30_000
        {
            bail!("diagnostics bounds, intervals, TTL, and scan budget are invalid");
        }
        if diagnostics.rule_version.trim().is_empty() || diagnostics.rule_version.len() > 128 {
            bail!("diagnostics.rule_version must be non-blank and at most 128 bytes");
        }
        if !diagnostics.audit_enabled || diagnostics.audit_path.trim().is_empty() {
            bail!(
                "enabled diagnostics require durable audit_enabled=true and a non-blank audit_path"
            );
        }
        if diagnostics.restricted_probe_enabled {
            validate_probe_startup(diagnostics, &NativeProbeContainment)
                .map_err(|error| anyhow::anyhow!(error))?;
        }
        if diagnostics.allow_consensus_restart {
            bail!(
                "diagnostics.allow_consensus_restart is unsupported until the restart safety controller is implemented"
            );
        }
    }

    if rpc_hardened
        && (node_config
            .rpc
            .rpc_user
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
            || node_config
                .rpc
                .rpc_pass
                .as_deref()
                .is_none_or(|value| value.trim().is_empty()))
    {
        bail!("--rpc-hardened requires non-blank rpc_user and rpc_pass (set via config or env)");
    }

    if node_config.rpc.enabled && !node_config.rpc.auth_enabled {
        let bind = node_config
            .rpc
            .bind_address
            .as_deref()
            .unwrap_or("127.0.0.1");
        if is_public_bind(bind) {
            // A03: refuse public bind without auth — wallet RPCs remain callable
            // when BasicAuth is unset; a warning alone is not an acceptable control.
            bail!(
                "RPC is enabled on non-loopback address '{}' without auth; set rpc.auth_enabled with rpc_user/rpc_pass, use --rpc-hardened, or bind to loopback",
                bind
            );
        }
    }

    if let Some(name) = backend_name {
        let normalized = name.trim().to_ascii_lowercase();
        let requires_path = matches!(
            normalized.as_str(),
            "rocksdb" | "rocksdbstore" | "rocksdb-store"
        );
        if requires_path && storage_path.map(|p| p.trim().is_empty()).unwrap_or(true) {
            bail!(
                "storage backend '{}' requires a data path (--storage or [storage.path])",
                name
            );
        }
    }

    if let Some(path) = storage_path {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let candidate = Path::new(trimmed);
            if candidate.is_file() {
                bail!(
                    "storage path '{}' is a file; provide a directory path",
                    trimmed
                );
            }
        }
    }

    if let Some(canonical) = node_config
        .network
        .network_type
        .as_deref()
        .and_then(infer_magic_from_type)
        && canonical != protocol_settings.network
    {
        warn!(
            target: "neo",
            network_type = ?node_config.network.network_type,
            configured_magic = format_args!("0x{:08x}", protocol_settings.network),
            canonical_magic = format_args!("0x{:08x}", canonical),
            "network type and magic differ; ensure this is intentional"
        );
    }

    let _ = node_config.oracle_service_settings(protocol_settings)?;
    let dbft_enabled = node_config.dbft_settings(protocol_settings)?.is_some();
    validate_signer_service_config(node_config, dbft_enabled)?;

    Ok(())
}

fn validate_signer_service_config(node_config: &NodeConfig, dbft_enabled: bool) -> Result<()> {
    let state_signing_enabled = node_config
        .state_service
        .as_ref()
        .is_some_and(|settings| settings.enabled && settings.auto_verify.unwrap_or(false));
    let oracle_signing_enabled = node_config
        .oracle_service
        .as_ref()
        .is_some_and(|settings| settings.enabled);
    let Some(config) = node_config.signer_service.as_ref() else {
        if dbft_enabled || state_signing_enabled || oracle_signing_enabled {
            bail!("dBFT, state verification, and Oracle signing require signer_service");
        }
        return Ok(());
    };
    if !config.enabled {
        if dbft_enabled || state_signing_enabled || oracle_signing_enabled {
            bail!(
                "dBFT, state verification, and Oracle signing require signer_service.enabled=true"
            );
        }
        return Ok(());
    }

    if config.state_path.trim().is_empty()
        || config.policy_version.trim().is_empty()
        || config.request_timeout_ms == 0
        || config.quota_limit_units == 0
        || config.quota_window_ms <= 0
    {
        bail!(
            "signer_service requires state_path, policy_version, positive request_timeout_ms, quota_limit_units, and quota_window_ms"
        );
    }

    let enabled: Vec<_> = config
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .collect();
    if enabled.is_empty() {
        bail!("signer_service.enabled requires at least one enabled provider");
    }

    let defaults: Vec<_> = enabled.iter().filter(|provider| provider.default).collect();
    if defaults.len() != 1 {
        bail!(
            "signer_service requires exactly one enabled default provider; found {}",
            defaults.len()
        );
    }

    let mut ids = BTreeSet::new();
    let mut account_purposes = BTreeSet::new();
    for provider in &enabled {
        if provider.id.trim().is_empty() {
            bail!("signer provider id must not be empty");
        }
        if !ids.insert(provider.id.as_str()) {
            bail!("duplicate signer provider id '{}'", provider.id);
        }
        if provider.purposes.is_empty()
            || provider.canonical_encodings.is_empty()
            || provider.algorithms.is_empty()
            || provider.keys.is_empty()
        {
            bail!(
                "signer provider '{}' must declare purposes, canonical_encodings, algorithms, and versioned keys",
                provider.id
            );
        }
        let mut versioned_keys = BTreeSet::new();
        for key in &provider.keys {
            let public_key = hex::decode(&key.public_key_hex).with_context(|| {
                format!(
                    "signer provider '{}' key '{}' has invalid public_key_hex",
                    provider.id, key.key_id
                )
            })?;
            let account = neo_core::UInt160::from_str(&key.account).with_context(|| {
                format!(
                    "signer provider '{}' key '{}' has invalid account",
                    provider.id, key.key_id
                )
            })?;
            let public_key_valid = ECPoint::new(ECCurve::Secp256r1, public_key.clone()).is_ok();
            let verification_script = ContractHelper::signature_redeem_script(&public_key);
            let public_key_account = neo_core::UInt160::from(Crypto::hash160(&verification_script));
            if key.key_id.trim().is_empty()
                || key.version == 0
                || key.account.trim().is_empty()
                || key.purposes.is_empty()
                || public_key.len() != 33
                || !public_key_valid
                || account.to_string() != key.account
                || account != public_key_account
                || !versioned_keys.insert((key.key_id.as_str(), key.version))
                || key
                    .purposes
                    .iter()
                    .any(|purpose| !provider.purposes.contains(purpose))
            {
                bail!(
                    "signer provider '{}' requires unique versioned keys with canonical accounts, valid compressed public keys bound to those accounts, and supported purposes",
                    provider.id
                );
            }
            for purpose in &key.purposes {
                if !account_purposes.insert((key.account.clone(), purpose.clone())) {
                    bail!(
                        "signer account '{}' purpose '{:?}' has more than one configured KeyRef",
                        key.account,
                        purpose
                    );
                }
            }
        }
        if provider.purposes.iter().any(|purpose| {
            !matches!(
                purpose,
                neo_core::signing::SignPurpose::Transaction
                    | neo_core::signing::SignPurpose::Consensus
                    | neo_core::signing::SignPurpose::StateRoot
                    | neo_core::signing::SignPurpose::OracleResponse
            )
        }) {
            bail!(
                "signer provider '{}' declares a purpose whose production entry point is not migrated",
                provider.id
            );
        }
        if provider
            .canonical_encodings
            .iter()
            .any(|encoding| encoding != NEO_SIGN_DATA_ENCODING)
        {
            bail!(
                "signer provider '{}' supports only canonical encoding '{}' in this release",
                provider.id,
                NEO_SIGN_DATA_ENCODING
            );
        }
        if provider
            .algorithms
            .iter()
            .any(|algorithm| algorithm != neo_core::signing::ALGORITHM_ECDSA_P256_SHA256)
        {
            bail!(
                "signer provider '{}' declares an unsupported signature algorithm",
                provider.id
            );
        }
        let declared_level = provider.security_level.ok_or_else(|| {
            anyhow::anyhow!(
                "signer provider '{}' must declare security_level",
                provider.id
            )
        })?;
        let valid_level = match provider.kind {
            SignerProviderKind::Nep6 => {
                declared_level == neo_core::signing::SecurityLevel::Software
            }
            SignerProviderKind::Hsm => matches!(
                declared_level,
                neo_core::signing::SecurityLevel::Software | neo_core::signing::SecurityLevel::Hsm
            ),
            SignerProviderKind::HostSealedTee => {
                declared_level == neo_core::signing::SecurityLevel::HostSealed
            }
            SignerProviderKind::HardwareTee => bail!(
                "hardware TEE signer provider '{}' is unavailable until a hardware-attested adapter is implemented",
                provider.id
            ),
            SignerProviderKind::Remote => {
                validate_remote_provider_config(provider)?;
                if declared_level != neo_core::signing::SecurityLevel::Software {
                    bail!(
                        "remote signer provider '{}' may only claim software security without hardware attestation",
                        provider.id
                    );
                }
                if provider
                    .remote
                    .as_ref()
                    .and_then(|remote| remote.attestation.as_ref())
                    .is_some()
                {
                    bail!(
                        "remote signer provider '{}' must not configure hardware attestation or claim a TEE boundary",
                        provider.id
                    );
                }
                true
            }
            SignerProviderKind::RemoteTee => {
                validate_remote_provider_config(provider)?;
                if declared_level != neo_core::signing::SecurityLevel::RemoteHardwareTee {
                    bail!(
                        "remote TEE signer provider '{}' must declare remote_hardware_tee security",
                        provider.id
                    );
                }
                let attestation = provider
                    .remote
                    .as_ref()
                    .and_then(|remote| remote.attestation.as_ref())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "remote TEE signer provider '{}' requires an explicit attestation policy",
                            provider.id
                        )
                    })?;
                let measurement = hex::decode(&attestation.measurement_hex).with_context(|| {
                    format!(
                        "remote TEE signer provider '{}' has invalid measurement_hex",
                        provider.id
                    )
                })?;
                let signer_measurement = attestation
                    .signer_measurement_hex
                    .as_deref()
                    .map(hex::decode)
                    .transpose()
                    .with_context(|| {
                        format!(
                            "remote TEE signer provider '{}' has invalid signer_measurement_hex",
                            provider.id
                        )
                    })?;
                if attestation.verifier.trim().is_empty()
                    || measurement.len() != 32
                    || signer_measurement
                        .as_ref()
                        .is_some_and(|value| value.len() != 32)
                    || attestation.product_id == 0
                    || attestation.minimum_security_version == 0
                    || attestation.max_challenge_age_ms == 0
                    || attestation.max_clock_skew_ms > MAX_ATTESTATION_CLOCK_SKEW_MS
                    || attestation.max_session_age_ms == 0
                {
                    bail!(
                        "remote TEE signer provider '{}' requires a named verifier, 32-byte measurements, product ID, minimum SVN, positive freshness limits, and clock skew no greater than {} ms",
                        provider.id,
                        MAX_ATTESTATION_CLOCK_SKEW_MS
                    );
                }
                true
            }
        };
        if !valid_level {
            bail!(
                "signer provider '{}' declares security_level '{:?}' incompatible with type '{:?}'",
                provider.id,
                declared_level,
                provider.kind
            );
        }
    }

    let default = defaults[0];
    if node_config.rpc.enabled
        && default
            .purposes
            .contains(&neo_core::signing::SignPurpose::Transaction)
        && (!node_config.rpc.auth_enabled
            || node_config
                .rpc
                .rpc_user
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            || node_config
                .rpc
                .rpc_pass
                .as_deref()
                .is_none_or(|value| value.trim().is_empty()))
    {
        bail!(
            "RPC transaction signing through signer_service requires enabled authentication with non-blank credentials"
        );
    }
    if node_config.rpc.enabled
        && !default
            .purposes
            .contains(&neo_core::signing::SignPurpose::Transaction)
    {
        bail!(
            "RPC wallet signing requires the default signer provider '{}' to support transaction purpose",
            default.id
        );
    }
    if dbft_enabled
        && !default
            .purposes
            .contains(&neo_core::signing::SignPurpose::Consensus)
    {
        bail!(
            "dBFT requires the default signer provider '{}' to support consensus purpose",
            default.id
        );
    }
    if state_signing_enabled
        && !default
            .purposes
            .contains(&neo_core::signing::SignPurpose::StateRoot)
    {
        bail!(
            "state service verification requires the default signer provider '{}' to support state_root purpose",
            default.id
        );
    }
    if oracle_signing_enabled
        && !default
            .purposes
            .contains(&neo_core::signing::SignPurpose::OracleResponse)
    {
        bail!(
            "OracleService requires the default signer provider '{}' to support oracle_response purpose",
            default.id
        );
    }
    if let Some(neofs) = node_config
        .oracle_service
        .as_ref()
        .and_then(|settings| settings.neofs.as_ref())
        && (neofs.auto_sign_bearer.unwrap_or(false)
            || neofs.wallet_connect.unwrap_or(false)
            || neofs.use_grpc.unwrap_or(false))
    {
        bail!(
            "NeoFS wallet signing, WalletConnect, and gRPC are disabled fail-closed until their dedicated signer algorithms are migrated"
        );
    }
    if let Some(required) = config.required_security_level
        && default.security_level != Some(required)
    {
        bail!(
            "default signer provider '{}' does not meet required security level '{:?}'; implicit downgrade is forbidden",
            default.id,
            required
        );
    }
    for purpose in &config.required_purposes {
        if !default.purposes.contains(purpose) {
            bail!(
                "default signer provider '{}' lacks required purpose '{:?}'; implicit downgrade is forbidden",
                default.id,
                purpose
            );
        }
    }
    for encoding in &config.required_canonical_encodings {
        if !default.canonical_encodings.contains(encoding) {
            bail!(
                "default signer provider '{}' lacks required canonical encoding '{}'",
                default.id,
                encoding
            );
        }
    }
    for algorithm in &config.required_algorithms {
        if !default.algorithms.contains(algorithm) {
            bail!(
                "default signer provider '{}' lacks required algorithm '{}'",
                default.id,
                algorithm
            );
        }
    }

    Ok(())
}

pub(crate) fn check_storage_access(
    backend: Option<&str>,
    storage_path: Option<&str>,
    storage_config: StorageConfig,
) -> Result<()> {
    let provider = select_store_provider(backend, storage_config)?;
    let Some(provider) = provider else {
        info!(target: "neo", "storage check: memory backend selected; nothing to validate");
        return Ok(());
    };

    let path = storage_path
        .ok_or_else(|| anyhow::anyhow!("storage check: no path provided for backend"))?;

    let store = provider
        .get_store(path)
        .map_err(|err| anyhow::anyhow!("storage check: failed to open store at {path}: {err}"))?;
    drop(store);
    info!(target: "neo", path, "storage check: backend opened successfully");
    Ok(())
}

#[allow(clippy::useless_vec)]
pub(crate) fn build_feature_summary() -> String {
    #[allow(unused_mut)]
    let mut features = vec!["plugins: rpc-server,tokens-tracker,application-logs"];

    #[cfg(feature = "full")]
    features.push("storage: rocksdb");

    #[cfg(feature = "tee")]
    features.push("tee: enabled");

    #[cfg(feature = "tee-sgx")]
    features.push("tee-sgx: hardware");

    features.join("; ")
}

pub(crate) fn build_state_service_settings(
    cli: &NodeCli,
    node_config: &NodeConfig,
    storage_path: Option<&str>,
    protocol_settings: &ProtocolSettings,
) -> Result<Option<StateServiceSettings>> {
    if cli.state_root {
        let default_state_dir = PathBuf::from("StateRoot");
        let requested_state_dir = cli
            .state_root_path
            .clone()
            .unwrap_or_else(|| default_state_dir.clone());
        let mut state_path = requested_state_dir.to_string_lossy().to_string();
        if state_path.contains("{0}") {
            state_path = state_path.replace("{0}", &format!("{:08X}", protocol_settings.network));
        }
        let resolved_state_dir = if PathBuf::from(&state_path).is_absolute() {
            PathBuf::from(&state_path)
        } else if let Some(storage_root) = storage_path {
            PathBuf::from(storage_root).join(state_path)
        } else {
            PathBuf::from(state_path)
        };
        let state_path = resolved_state_dir.to_string_lossy().to_string();
        // `--state-root-full-state` can only enable full state; it never silently
        // overrides `full_state = true` from the configuration.
        let full_state = cli.state_root_full_state
            || node_config
                .state_service_settings(protocol_settings)?
                .is_some_and(|settings| settings.full_state);
        info!(
            target: "neo",
            path = %state_path,
            full_state,
            "state root calculation enabled"
        );
        return Ok(Some(StateServiceSettings {
            full_state,
            path: state_path,
            network: protocol_settings.network,
            auto_verify: false,
            max_find_result_items: 100,
            exception_policy: UnhandledExceptionPolicy::StopPlugin,
        }));
    }

    let Some(mut settings) = node_config.state_service_settings(protocol_settings)? else {
        return Ok(None);
    };

    if settings.network != protocol_settings.network {
        warn!(
            target: "neo",
            configured = format_args!("0x{:08x}", settings.network),
            expected = format_args!("0x{:08x}", protocol_settings.network),
            "state service network mismatch; skipping"
        );
        return Ok(None);
    }

    let store_path = crate::config::resolve_state_service_store_path(&settings);
    settings.path = store_path.to_string_lossy().to_string();

    info!(
        target: "neo",
        path = %settings.path,
        full_state = settings.full_state,
        auto_verify = settings.auto_verify,
        "state root calculation enabled"
    );
    Ok(Some(settings))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(test)]
    use zeroize::Zeroizing;

    #[test]
    fn validate_requires_storage_path_for_rocksdb() {
        let mut cfg = NodeConfig::default();
        cfg.storage.backend = Some("rocksdb".into());
        let err =
            validate_node_config(&cfg, None, Some("rocksdb"), &cfg.protocol_settings(), false)
                .expect_err("should fail without storage path");
        assert!(
            err.to_string()
                .to_ascii_lowercase()
                .contains("storage backend"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_accepts_memory_without_path() {
        let cfg = NodeConfig::default();
        validate_node_config(&cfg, None, Some("memory"), &cfg.protocol_settings(), false)
            .expect("memory backend should not require path");
    }

    #[test]
    fn validate_enforces_rpc_auth_credentials() {
        let mut cfg = NodeConfig::default();
        cfg.rpc.enabled = true;
        cfg.rpc.auth_enabled = true;
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("missing rpc credentials should error");
        assert!(
            err.to_string().to_ascii_lowercase().contains("rpc_user"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_rejects_storage_path_that_is_file() {
        let tmp = tempfile::NamedTempFile::new().expect("temp file");
        let path_str = tmp.path().to_string_lossy().to_string();

        let mut cfg = NodeConfig::default();
        cfg.storage.backend = Some("rocksdb".into());
        let err = validate_node_config(
            &cfg,
            Some(&path_str),
            Some("rocksdb"),
            &cfg.protocol_settings(),
            false,
        )
        .expect_err("file path should be rejected");
        assert!(
            err.to_string().to_ascii_lowercase().contains("file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_hardened_requires_credentials() {
        let cfg = NodeConfig::default();
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), true)
            .expect_err("hardened mode without credentials should fail");
        assert!(
            err.to_string().to_ascii_lowercase().contains("rpc_user"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_rejects_public_bind_without_auth() {
        let mut cfg = NodeConfig::default();
        cfg.rpc.enabled = true;
        cfg.rpc.auth_enabled = false;
        cfg.rpc.bind_address = Some("0.0.0.0".to_string());

        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("public bind without auth must be rejected");
        assert!(
            err.to_string()
                .to_ascii_lowercase()
                .contains("without auth"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_allows_loopback_without_auth() {
        let mut cfg = NodeConfig::default();
        cfg.rpc.enabled = true;
        cfg.rpc.auth_enabled = false;
        cfg.rpc.bind_address = Some("127.0.0.1".to_string());

        validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect("loopback without auth should be allowed");
    }

    #[test]
    fn validate_rejects_blank_rpc_credentials() {
        let mut cfg = NodeConfig::default();
        cfg.rpc.enabled = true;
        cfg.rpc.auth_enabled = true;
        cfg.rpc.rpc_user = Some(Zeroizing::new("   ".to_string()));
        cfg.rpc.rpc_pass = Some(Zeroizing::new("password".to_string()));
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("blank RPC username must fail");
        assert!(err.to_string().contains("non-blank"));

        cfg.rpc.rpc_user = Some(Zeroizing::new("user".to_string()));
        cfg.rpc.rpc_pass = Some(Zeroizing::new("\t".to_string()));
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("blank RPC password must fail");
        assert!(err.to_string().contains("non-blank"));
    }

    #[test]
    fn validate_rejects_default_rpc_credentials_on_public_bind() {
        let mut cfg = NodeConfig::default();
        cfg.rpc.enabled = true;
        cfg.rpc.auth_enabled = true;
        cfg.rpc.bind_address = Some("0.0.0.0".to_string());
        cfg.rpc.rpc_user = Some(Zeroizing::new("neo".to_string()));
        cfg.rpc.rpc_pass = Some(Zeroizing::new("change-me-mainnet-rpc-password".to_string()));

        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("template credentials on public bind must be rejected");
        assert!(
            err.to_string()
                .to_ascii_lowercase()
                .contains("default rpc credentials"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_rejects_unsupported_max_free_transactions_per_block() {
        let mut cfg = NodeConfig::default();
        cfg.blockchain.max_free_transactions_per_block = Some(20);
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("unsupported blockchain.max_free_transactions_per_block should fail");
        assert!(
            err.to_string()
                .contains("blockchain.max_free_transactions_per_block"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_allows_mempool_max_transactions_per_sender_for_compat() {
        let cfg = NodeConfig {
            mempool: Some(crate::config::MempoolSection {
                max_transactions: Some(10_000),
                max_transactions_per_sender: Some(100),
            }),
            ..Default::default()
        };
        validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect("mempool.max_transactions_per_sender should be accepted for compatibility");
    }

    #[test]
    fn diagnostics_probe_is_disabled_by_default() {
        let diagnostics = crate::config::DiagnosticsSection::default();
        assert!(!diagnostics.restricted_probe_enabled);
        assert!(!diagnostics.allow_transaction_quarantine);
        assert_eq!(diagnostics.quarantine_ttl_ms, 30_000);
        assert_eq!(diagnostics.restricted_probe_queue_capacity, 16);
        assert_eq!(diagnostics.restricted_probe_concurrency, 1);
        assert_eq!(diagnostics.restricted_probe_run_timeout_ms, 3_000);
        assert_eq!(diagnostics.restricted_probe_suite_timeout_ms, 7_000);
        assert_eq!(
            diagnostics.restricted_probe_memory_limit_bytes,
            256 * 1024 * 1024
        );
        assert_eq!(diagnostics.restricted_probe_frame_limit_bytes, 512 * 1024);
    }

    #[test]
    fn diagnostics_probe_requires_diagnostics_enabled() {
        let cfg = NodeConfig {
            diagnostics: Some(crate::config::DiagnosticsSection {
                restricted_probe_enabled: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let error = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("probe cannot bypass the diagnostics startup gate");
        assert!(
            error
                .to_string()
                .contains("requires diagnostics.enabled=true")
        );
    }

    #[test]
    fn diagnostics_probe_rejects_out_of_range_budgets_before_capability_check() {
        let mut cases = Vec::new();
        let mut diagnostics = crate::config::DiagnosticsSection::default();
        diagnostics.restricted_probe_queue_capacity = 17;
        cases.push(diagnostics);
        let mut diagnostics = crate::config::DiagnosticsSection::default();
        diagnostics.restricted_probe_concurrency = 2;
        cases.push(diagnostics);
        let mut diagnostics = crate::config::DiagnosticsSection::default();
        diagnostics.restricted_probe_run_timeout_ms = 3_001;
        cases.push(diagnostics);
        let mut diagnostics = crate::config::DiagnosticsSection::default();
        diagnostics.restricted_probe_suite_timeout_ms = 7_001;
        cases.push(diagnostics);
        let mut diagnostics = crate::config::DiagnosticsSection::default();
        diagnostics.restricted_probe_memory_limit_bytes -= 1;
        cases.push(diagnostics);
        let mut diagnostics = crate::config::DiagnosticsSection::default();
        diagnostics.restricted_probe_frame_limit_bytes += 1;
        cases.push(diagnostics);

        for mut diagnostics in cases {
            diagnostics.enabled = true;
            diagnostics.restricted_probe_enabled = true;
            let cfg = NodeConfig {
                diagnostics: Some(diagnostics),
                ..Default::default()
            };
            let error = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
                .expect_err("out-of-range probe budget must fail startup validation");
            assert!(
                error
                    .to_string()
                    .contains("invalid restricted probe configuration")
            );
        }
    }

    #[test]
    fn diagnostics_probe_configuration_has_no_arguments_surface() {
        let error = toml::from_str::<NodeConfig>(
            "[diagnostics]\nrestricted_probe_args = [\"--url\", \"https://example.invalid\"]\n",
        )
        .expect_err("arbitrary probe arguments must not be accepted by configuration");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn diagnostics_probe_native_capability_fails_closed() {
        let directory = tempfile::tempdir().expect("temp dir");
        let executable = directory.path().join(if cfg!(windows) {
            "neo-probe.exe"
        } else {
            "neo-probe"
        });
        fs::write(&executable, b"test worker").expect("worker fixture");
        let cfg = NodeConfig {
            diagnostics: Some(crate::config::DiagnosticsSection {
                enabled: true,
                restricted_probe_enabled: true,
                restricted_probe_worker_path: executable.to_string_lossy().into_owned(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let error = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("native capability must reject unproven hard isolation");
        assert!(error.to_string().contains("containment is unsupported"));
    }

    #[test]
    fn diagnostics_rejects_consensus_restart() {
        let cfg = NodeConfig {
            diagnostics: Some(crate::config::DiagnosticsSection {
                enabled: true,
                allow_consensus_restart: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("unimplemented consensus restart must fail closed");
    }

    #[test]
    fn validate_rejects_zero_mempool_max_transactions_per_sender() {
        let cfg = NodeConfig {
            mempool: Some(crate::config::MempoolSection {
                max_transactions: Some(10_000),
                max_transactions_per_sender: Some(0),
            }),
            ..Default::default()
        };
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("mempool.max_transactions_per_sender=0 should be rejected");
        assert!(
            err.to_string()
                .contains("mempool.max_transactions_per_sender"),
            "unexpected error: {err}"
        );
    }

    fn signer_config(provider_blocks: &str, requirements: &str) -> NodeConfig {
        let mut config: NodeConfig = toml::from_str(&format!(
            "[signer_service]\nenabled = true\nstate_path = \"target/signer-service-tests\"\nquota_limit_units = 100\n{requirements}\n{provider_blocks}"
        ))
        .expect("parse signer config");
        let key_pair =
            neo_core::wallets::KeyPair::from_private_key(&[0x11; 32]).expect("signer fixture key");
        if let Some(service) = config.signer_service.as_mut() {
            for (index, provider) in service.providers.iter_mut().enumerate() {
                provider.keys = vec![crate::config::SignerKeySection {
                    key_id: format!("fixture-key-{index}"),
                    version: 2,
                    public_key_hex: hex::encode(key_pair.public_key()),
                    account: key_pair.get_script_hash().to_string(),
                    purposes: provider.purposes.clone(),
                }];
            }
        }
        config
    }

    #[test]
    fn signer_config_rejects_multiple_defaults() {
        let cfg = signer_config(
            r#"
[[signer_service.providers]]
id = "wallet-a"
type = "nep6"
enabled = true
default = true
security_level = "software"
purposes = ["transaction"]
canonical_encodings = ["neo-n3-sign-data-v1"]
algorithms = ["ecdsa-p256-sha256"]

[[signer_service.providers]]
id = "wallet-b"
type = "nep6"
enabled = true
default = true
security_level = "software"
purposes = ["transaction"]
canonical_encodings = ["neo-n3-sign-data-v1"]
algorithms = ["ecdsa-p256-sha256"]
"#,
            "",
        );
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("multiple defaults must fail");
        assert!(err.to_string().contains("exactly one"));
    }

    #[test]
    fn signer_config_rejects_capability_downgrade() {
        let cfg = signer_config(
            r#"
[[signer_service.providers]]
id = "wallet"
type = "nep6"
enabled = true
default = true
security_level = "software"
purposes = ["transaction"]
canonical_encodings = ["neo-n3-sign-data-v1"]
algorithms = ["ecdsa-p256-sha256"]
"#,
            "required_security_level = \"hsm\"\nrequired_purposes = [\"consensus\"]",
        );
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("security downgrade must fail");
        assert!(err.to_string().contains("implicit downgrade"));
    }

    #[test]
    fn signer_config_rejects_remote_provider_without_mtls_configuration() {
        let cfg = signer_config(
            r#"
[[signer_service.providers]]
id = "remote"
type = "remote_tee"
enabled = true
default = true
security_level = "remote_hardware_tee"
endpoint = "https://signer.example"
purposes = ["transaction"]
canonical_encodings = ["neo-n3-sign-data-v1"]
algorithms = ["ecdsa-p256-sha256"]
"#,
            "",
        );
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("remote provider without TLS material must fail closed");
        assert!(err.to_string().contains("mTLS configuration"));
    }

    #[test]
    fn signer_config_rejects_plaintext_remote_endpoint() {
        let mut cfg = signer_config(
            r#"
[[signer_service.providers]]
id = "remote"
type = "remote"
enabled = true
default = true
security_level = "software"
endpoint = "https://signer.example"
purposes = ["transaction"]
canonical_encodings = ["neo-n3-sign-data-v1"]
algorithms = ["ecdsa-p256-sha256"]
"#,
            "",
        );
        cfg.signer_service.as_mut().unwrap().providers[0].endpoint =
            Some("http://signer.example".to_string());
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("plaintext remote signer endpoint must fail closed");
        assert!(err.to_string().contains("credential-free HTTPS"));
    }

    #[test]
    fn signer_config_rejects_default_without_dbft_capability() {
        let mut cfg = signer_config(
            r#"
[[signer_service.providers]]
id = "wallet"
type = "nep6"
enabled = true
default = true
security_level = "software"
purposes = ["transaction"]
canonical_encodings = ["neo-n3-sign-data-v1"]
algorithms = ["ecdsa-p256-sha256"]
"#,
            "",
        );
        cfg.dbft = Some(crate::config::DbftSection {
            enabled: true,
            ..Default::default()
        });
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("dBFT must require consensus capability on the default provider");
        assert!(err.to_string().contains("consensus purpose"));
    }

    #[test]
    fn signer_config_accepts_simulated_hsm_as_software_only() {
        let cfg = signer_config(
            r#"
[[signer_service.providers]]
id = "simulated-hsm"
type = "hsm"
enabled = true
default = true
security_level = "software"
purposes = ["consensus"]
canonical_encodings = ["neo-n3-sign-data-v1"]
algorithms = ["ecdsa-p256-sha256"]
"#,
            "required_security_level = \"software\"",
        );
        validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect("simulated HSM must be classified as software");
    }

    #[test]
    fn signer_config_accepts_exact_capability_match() {
        let cfg = signer_config(
            r#"
[[signer_service.providers]]
id = "wallet"
type = "nep6"
enabled = true
default = true
security_level = "software"
purposes = ["transaction"]
canonical_encodings = ["neo-n3-sign-data-v1"]
algorithms = ["ecdsa-p256-sha256"]
"#,
            "required_security_level = \"software\"\nrequired_purposes = [\"transaction\"]\nrequired_canonical_encodings = [\"neo-n3-sign-data-v1\"]\nrequired_algorithms = [\"ecdsa-p256-sha256\"]",
        );
        validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect("matching capabilities and account-bound version 2 key should pass");
        assert_eq!(
            cfg.signer_service.as_ref().unwrap().providers[0].keys[0].version,
            2
        );
    }

    #[test]
    fn state_root_cli_flag_keeps_configured_full_state() {
        use clap::Parser;

        let cfg: NodeConfig =
            toml::from_str("[state_service]\nenabled = true\nfull_state = true\n")
                .expect("parse config");
        let settings = cfg.protocol_settings();

        let cli = NodeCli::parse_from(["neo-node", "--state-root"]);
        let resolved = build_state_service_settings(&cli, &cfg, Some("data"), &settings)
            .expect("settings")
            .expect("state service enabled");
        assert!(resolved.full_state);

        let cli = NodeCli::parse_from(["neo-node", "--state-root"]);
        let resolved =
            build_state_service_settings(&cli, &NodeConfig::default(), Some("data"), &settings)
                .expect("settings")
                .expect("state service enabled");
        assert!(!resolved.full_state);
    }

    #[test]
    fn check_storage_allows_memory_without_path() {
        check_storage_access(Some("memory"), None, StorageConfig::default())
            .expect("memory backend should skip validation");
    }

    #[test]
    #[cfg(feature = "full")]
    fn check_storage_succeeds_with_rocksdb_path() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let db_path = tmp.path().join("rocksdb-check");
        let cfg = StorageConfig {
            path: db_path.clone(),
            ..Default::default()
        };

        check_storage_access(
            Some("rocksdb"),
            Some(db_path.to_string_lossy().as_ref()),
            cfg,
        )
        .expect("rocksdb backend should open successfully");
    }

    #[test]
    fn check_storage_network_writes_markers() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let path = tmp.path().join("store");
        let path_str = path.to_string_lossy().to_string();

        check_storage_network(&path_str, 0x1234_5678, false).expect("check storage network");

        let magic = fs::read_to_string(path.join("NETWORK_MAGIC")).expect("read magic");
        assert!(magic.contains("0x12345678"));

        let version = fs::read_to_string(path.join("VERSION")).expect("read version");
        assert_eq!(version.trim(), STORAGE_VERSION);
    }

    #[test]
    fn check_storage_network_readonly_requires_markers() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let path = tmp.path().join("store");
        fs::create_dir_all(&path).expect("create dir");
        let err = check_storage_network(path.to_str().unwrap(), 0x1, true)
            .expect_err("missing markers should fail in read-only");
        assert!(err.to_string().to_ascii_lowercase().contains("marker"));

        fs::write(path.join("NETWORK_MAGIC"), "0x00000001").expect("write magic");
        fs::write(path.join("VERSION"), STORAGE_VERSION).expect("write version");
        check_storage_network(path.to_str().unwrap(), 0x1, true)
            .expect("markers present should pass");
    }

    #[test]
    fn bundled_mainnet_config_is_check_config_valid() {
        let cfg: NodeConfig =
            toml::from_str(include_str!("../../../neo_mainnet_node.toml")).expect("parse mainnet");
        let settings = cfg.protocol_settings();
        validate_node_config(
            &cfg,
            cfg.storage.path.as_deref(),
            cfg.storage.backend.as_deref(),
            &settings,
            false,
        )
        .expect("bundled mainnet config should pass --check-config validation");
    }

    #[test]
    fn bundled_production_config_is_check_config_valid() {
        let cfg: NodeConfig = toml::from_str(include_str!("../../../neo_production_node.toml"))
            .expect("parse production");
        let settings = cfg.protocol_settings();
        validate_node_config(
            &cfg,
            cfg.storage.path.as_deref(),
            cfg.storage.backend.as_deref(),
            &settings,
            false,
        )
        .expect("bundled production config should pass --check-config validation");
    }
}
