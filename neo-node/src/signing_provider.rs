//! Unified signer providers and node-local signing runtime.

use crate::config::{SignerProviderKind, SignerServiceSection};
use crate::remote_signer::{
    MtlsRemoteSignerTransport, RemoteSignerProvider, validate_remote_provider_config,
};
use crate::wallet_provider::NodeWalletProvider;
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use neo_primitives::UInt160;
use neo_smart_contract::helper::Helper as ContractHelper;
use neo_wallets::signing::{
    ALGORITHM_ECDSA_P256_SHA256, AuthenticatedContext, AuthorizedSignRequest, DefaultSignerService,
    DurableSigningStore, KeyInfo, KeyRef, PolicyLimits, ProviderCapabilities,
    ProviderOutcomeProvenance, ProviderSignature, RemoteSignerTransport, SIGNING_SCHEMA_VERSION,
    SecurityLevel, SignError, SignErrorCode, SignPayload, SignPurpose, SignResult, SignerProvider,
    SignerProviderRegistry, SignerService, SigningKeyBinding, SigningKeyResolver, SigningPolicy,
    SigningStore, TrustedSignerClient,
};
use neo_wallets::{Wallet, WalletProvider};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

pub(crate) const NEO_SIGN_DATA_ENCODING: &str = "neo-n3-sign-data-v1";
pub(crate) const RPC_SIGNING_PRINCIPAL: &str = "neo-rpc-wallet";
pub(crate) const STATE_SIGNING_PRINCIPAL: &str = "neo-state-service";
pub(crate) const ORACLE_SIGNING_PRINCIPAL: &str = "neo-oracle-service";
pub(crate) const NODE_SIGNING_TENANT: &str = "neo-node";

/// Runtime signer backend selected by node startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActiveSignerBackend {
    pub(crate) kind: SignerProviderKind,
    pub(crate) security_level: SecurityLevel,
}

/// Shared signer service and immutable entry-point configuration.
#[derive(Clone)]
pub(crate) struct NodeSignerRuntime {
    service: Arc<dyn SignerService>,
    default_provider: String,
    policy_version: String,
    request_timeout: Duration,
    key_resolver: Arc<SigningKeyResolver>,
    purposes: BTreeSet<SignPurpose>,
}

impl std::fmt::Debug for NodeSignerRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeSignerRuntime")
            .field("default_provider", &self.default_provider)
            .field("policy_version", &self.policy_version)
            .field("request_timeout", &self.request_timeout)
            .finish_non_exhaustive()
    }
}

impl NodeSignerRuntime {
    pub(crate) fn service(&self) -> Arc<dyn SignerService> {
        Arc::clone(&self.service)
    }

    pub(crate) fn default_provider(&self) -> &str {
        &self.default_provider
    }

    pub(crate) fn policy_version(&self) -> &str {
        &self.policy_version
    }

    pub(crate) fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub(crate) fn key_resolver(&self) -> Arc<SigningKeyResolver> {
        Arc::clone(&self.key_resolver)
    }

    pub(crate) fn ensure_purpose(&self, purpose: &SignPurpose) -> Result<()> {
        if self.purposes.contains(purpose) {
            return Ok(());
        }
        bail!(
            "unified signer policy '{}' has no key authorized for purpose {:?}",
            self.policy_version,
            purpose
        )
    }

    pub(crate) fn trusted_client(
        &self,
        principal: &str,
        authentication_method: &str,
        scope: &str,
    ) -> SignResult<TrustedSignerClient> {
        TrustedSignerClient::new(
            self.service(),
            self.default_provider.clone(),
            self.policy_version.clone(),
            self.request_timeout,
            AuthenticatedContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                principal: principal.to_string(),
                tenant: NODE_SIGNING_TENANT.to_string(),
                authentication_method: authentication_method.to_string(),
                scopes: BTreeSet::from([scope.to_string()]),
                claims: BTreeMap::new(),
            },
            Arc::clone(&self.key_resolver),
        )
    }
}

struct WalletSignerProvider {
    wallet: Arc<dyn Wallet>,
    capabilities: ProviderCapabilities,
    keys: BTreeMap<KeyRef, (UInt160, Vec<u8>)>,
}

impl WalletSignerProvider {
    fn new(
        wallet: Arc<dyn Wallet>,
        provider: &crate::config::SignerProviderSection,
    ) -> Result<Self> {
        let mut keys = BTreeMap::new();
        for key in &provider.keys {
            let account = UInt160::from_str(&key.account).with_context(|| {
                format!(
                    "signer provider '{}' key '{}' has invalid account",
                    provider.id, key.key_id
                )
            })?;
            let public_key = hex::decode(&key.public_key_hex).with_context(|| {
                format!(
                    "signer provider '{}' key '{}' has invalid public key",
                    provider.id, key.key_id
                )
            })?;
            keys.insert(
                KeyRef::new(&provider.id, &key.key_id, key.version),
                (account, public_key),
            );
        }
        Ok(Self {
            wallet,
            keys,
            capabilities: ProviderCapabilities {
                provider: provider.id.clone(),
                purposes: provider.purposes.iter().cloned().collect(),
                canonical_encodings: provider.canonical_encodings.iter().cloned().collect(),
                algorithms: provider.algorithms.iter().cloned().collect(),
                security_level: provider
                    .security_level
                    .context("validated signer provider security level is missing")?,
                outcome_provenance: ProviderOutcomeProvenance::DeterministicLocal,
            },
        })
    }

    fn public_key_for(&self, key_ref: &KeyRef) -> SignResult<(UInt160, Vec<u8>)> {
        let (script_hash, configured_public_key) =
            self.keys.get(key_ref).cloned().ok_or_else(|| {
                SignError::new(
                    SignErrorCode::KeyNotFound,
                    "signer key id and version are not configured for the wallet provider",
                )
            })?;
        let account = self.wallet.get_account(&script_hash).ok_or_else(|| {
            SignError::new(SignErrorCode::KeyNotFound, "wallet account was not found")
        })?;
        if account.is_locked() || !account.has_key() {
            return Err(SignError::new(
                SignErrorCode::KeyDisabled,
                "wallet account is locked or has no signing key",
            ));
        }
        let contract = account.contract().ok_or_else(|| {
            SignError::new(
                SignErrorCode::CapabilityUnavailable,
                "wallet signer requires an explicit signature contract",
            )
        })?;
        if !ContractHelper::is_signature_contract(&contract.script) || contract.script.len() < 35 {
            return Err(SignError::new(
                SignErrorCode::CapabilityUnavailable,
                "wallet signer only supports standard single-signature contracts",
            ));
        }
        let wallet_public_key = contract.script[2..35].to_vec();
        if wallet_public_key != configured_public_key {
            return Err(SignError::new(
                SignErrorCode::VerificationFailed,
                "wallet account public key does not match configured KeyRef binding",
            ));
        }
        Ok((script_hash, wallet_public_key))
    }
}

#[async_trait]
impl SignerProvider for WalletSignerProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }

    async fn key_info(&self, key_ref: &KeyRef) -> SignResult<KeyInfo> {
        let (_, public_key) = self.public_key_for(key_ref)?;
        Ok(KeyInfo {
            schema_version: SIGNING_SCHEMA_VERSION,
            key_ref: key_ref.clone(),
            algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
            public_key,
            security_level: self.capabilities.security_level,
            enabled: true,
            attributes: BTreeMap::from([("wallet".to_string(), self.wallet.name().to_string())]),
        })
    }

    async fn sign_authorized(
        &self,
        request: &AuthorizedSignRequest,
    ) -> SignResult<ProviderSignature> {
        let bytes = match request.payload() {
            SignPayload::Canonical { encoding, bytes } if encoding == NEO_SIGN_DATA_ENCODING => {
                bytes
            }
            SignPayload::Canonical { .. } => {
                return Err(SignError::new(
                    SignErrorCode::CapabilityUnavailable,
                    "wallet provider rejected an unsupported canonical encoding",
                ));
            }
            SignPayload::Digest { .. } => {
                return Err(SignError::new(
                    SignErrorCode::CapabilityUnavailable,
                    "wallet provider does not accept pre-hashed payloads",
                ));
            }
        };
        let (script_hash, _) = self.public_key_for(request.key_ref())?;
        let signature = self.wallet.sign(bytes, &script_hash).map_err(|error| {
            SignError::new(
                SignErrorCode::ProviderFailure,
                format!("wallet signing failed: {error}"),
            )
        })?;
        if signature.len() != 64 {
            return Err(SignError::new(
                SignErrorCode::ProviderFailure,
                "wallet provider returned a non-P-256 signature",
            ));
        }
        Ok(ProviderSignature {
            algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
            signature,
        })
    }
}

fn validate_service_config(config: &SignerServiceSection) -> Result<()> {
    if config.state_path.trim().is_empty()
        || config.policy_version.trim().is_empty()
        || config.request_timeout_ms == 0
        || config.quota_limit_units == 0
        || config.quota_window_ms <= 0
    {
        bail!(
            "enabled signer service requires a durable state path, policy version, positive timeout and quota policy"
        );
    }
    let enabled: Vec<_> = config
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .collect();
    if enabled.is_empty() || enabled.iter().filter(|provider| provider.default).count() != 1 {
        bail!("enabled signer service requires providers and exactly one default provider");
    }
    let default = enabled
        .iter()
        .copied()
        .find(|provider| provider.default)
        .context("default signer provider is missing")?;
    for provider in &enabled {
        if provider.id.trim().is_empty()
            || provider.security_level.is_none()
            || provider.purposes.is_empty()
            || provider.canonical_encodings.is_empty()
            || provider.algorithms.is_empty()
            || provider.keys.is_empty()
        {
            bail!(
                "signer provider '{}' must declare identity, security, capabilities and keys",
                provider.id
            );
        }
        for key in &provider.keys {
            UInt160::from_str(&key.account).with_context(|| {
                format!(
                    "signer provider '{}' key '{}' has invalid account",
                    provider.id, key.key_id
                )
            })?;
            let public_key = hex::decode(&key.public_key_hex).with_context(|| {
                format!(
                    "signer provider '{}' key '{}' has invalid public key",
                    provider.id, key.key_id
                )
            })?;
            if key.key_id.trim().is_empty()
                || key.version == 0
                || key.purposes.is_empty()
                || public_key.len() != 33
            {
                bail!(
                    "signer provider '{}' key '{}' requires a name, nonzero version, purposes and compressed P-256 public key",
                    provider.id,
                    key.key_id
                );
            }
        }
    }
    let default_purposes: BTreeSet<_> = default.purposes.iter().cloned().collect();
    let default_encodings: BTreeSet<_> = default.canonical_encodings.iter().cloned().collect();
    let default_algorithms: BTreeSet<_> = default.algorithms.iter().cloned().collect();
    if !config
        .required_purposes
        .iter()
        .all(|purpose| default_purposes.contains(purpose))
        || !config
            .required_canonical_encodings
            .iter()
            .all(|encoding| default_encodings.contains(encoding))
        || !config
            .required_algorithms
            .iter()
            .all(|algorithm| default_algorithms.contains(algorithm))
        || config
            .required_security_level
            .is_some_and(|level| Some(level) != default.security_level)
    {
        bail!("default signer provider does not satisfy the required capability boundary");
    }
    Ok(())
}

pub(crate) fn initialize_signer_service(
    config: Option<&SignerServiceSection>,
    network: u32,
    wallet_provider: Option<&Arc<NodeWalletProvider>>,
    active_backend: Option<ActiveSignerBackend>,
) -> Result<Option<Arc<NodeSignerRuntime>>> {
    let Some(config) = config.filter(|config| config.enabled) else {
        return Ok(None);
    };
    validate_service_config(config)?;
    let enabled: Vec<_> = config
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .collect();
    let default = enabled
        .iter()
        .copied()
        .find(|provider| provider.default)
        .context("validated signer configuration has no default provider")?;
    let has_local = enabled.iter().any(|provider| {
        !matches!(
            provider.kind,
            SignerProviderKind::Remote
                | SignerProviderKind::RemoteTee
                | SignerProviderKind::HardwareTee
        )
    });
    let wallet = if has_local {
        Some(
            wallet_provider
                .cloned()
                .context("local signer service requires an attached node wallet provider")?
                .get_wallet()
                .context("local signer service requires a loaded wallet")?,
        )
    } else {
        None
    };
    if has_local
        && wallet.as_ref().is_none_or(|wallet| {
            wallet
                .get_accounts()
                .iter()
                .all(|account| account.is_locked() || !account.has_key())
        })
    {
        bail!("signer service wallet has no unlocked signable accounts");
    }

    let mut registry = SignerProviderRegistry::new();
    for provider in &enabled {
        let registered: Arc<dyn SignerProvider> = match provider.kind {
            SignerProviderKind::Remote | SignerProviderKind::RemoteTee => {
                validate_remote_provider_config(provider)?;
                let transport: Arc<dyn RemoteSignerTransport> = Arc::new(
                    MtlsRemoteSignerTransport::from_config(provider).with_context(|| {
                        format!(
                            "failed to configure remote signer provider '{}'",
                            provider.id
                        )
                    })?,
                );
                Arc::new(RemoteSignerProvider::new(
                    provider,
                    &config.policy_version,
                    transport,
                    None,
                )?)
            }
            SignerProviderKind::HardwareTee => {
                bail!(
                    "signer provider '{}' requests a locally attested hardware TEE, but no attested local TEE provider is available; refusing to substitute an HSM or software signer",
                    provider.id
                );
            }
            _ => {
                let active_backend = active_backend
                    .context("local signer service requires an active local backend")?;
                if provider.kind != active_backend.kind
                    || provider.security_level != Some(active_backend.security_level)
                {
                    bail!(
                        "signer provider '{}' requests {:?}/{:?}, but the active wallet is {:?}/{:?}; implicit provider or security downgrade is forbidden",
                        provider.id,
                        provider.kind,
                        provider.security_level,
                        active_backend.kind,
                        active_backend.security_level
                    );
                }
                Arc::new(WalletSignerProvider::new(
                    Arc::clone(wallet.as_ref().context("local wallet is missing")?),
                    provider,
                )?)
            }
        };
        registry
            .register(registered)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    }

    let mut key_bindings = Vec::new();
    for provider in &enabled {
        for key in &provider.keys {
            let public_key = hex::decode(&key.public_key_hex)?;
            let key_ref = KeyRef::new(&provider.id, &key.key_id, key.version);
            for purpose in &key.purposes {
                key_bindings.push(SigningKeyBinding {
                    account: key.account.clone(),
                    purpose: purpose.clone(),
                    key_ref: key_ref.clone(),
                    public_key: public_key.clone(),
                });
            }
        }
    }
    let key_resolver = Arc::new(
        SigningKeyResolver::new(key_bindings)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?,
    );
    let mut policy = SigningPolicy {
        version: config.policy_version.clone(),
        key_resolver: (*key_resolver).clone(),
        limits: PolicyLimits {
            max_fee_units: config.max_fee_units,
            quota_limit_units: config.quota_limit_units,
            quota_window_ms: config.quota_window_ms,
        },
        ..SigningPolicy::default()
    };
    policy.principals.allow.extend([
        RPC_SIGNING_PRINCIPAL.to_string(),
        STATE_SIGNING_PRINCIPAL.to_string(),
        ORACLE_SIGNING_PRINCIPAL.to_string(),
    ]);
    policy.tenants.allow.insert(NODE_SIGNING_TENANT.to_string());
    policy.networks.allow.insert(network);
    let mut purposes = BTreeSet::new();
    for provider in &enabled {
        let level = provider
            .security_level
            .context("validated provider security level is missing")?;
        policy.security_levels.allow.insert(level);
        policy
            .purposes
            .allow
            .extend(provider.purposes.iter().cloned());
        for key in &provider.keys {
            let key_ref = KeyRef::new(&provider.id, &key.key_id, key.version);
            policy.keys.allow.insert(key_ref.selector());
            policy.accounts.allow.insert(key.account.clone());
            for purpose in &key.purposes {
                purposes.insert(purpose.clone());
                let principal = match purpose {
                    SignPurpose::Transaction => RPC_SIGNING_PRINCIPAL,
                    SignPurpose::StateRoot => STATE_SIGNING_PRINCIPAL,
                    SignPurpose::OracleResponse => ORACLE_SIGNING_PRINCIPAL,
                    _ => continue,
                };
                policy
                    .authorizations
                    .allow
                    .insert(SigningPolicy::authorization_selector(
                        principal,
                        NODE_SIGNING_TENANT,
                        &key_ref,
                        network,
                        purpose,
                        &purpose.required_scope(),
                    ));
            }
        }
    }

    let store: Arc<dyn SigningStore> = Arc::new(
        DurableSigningStore::open(&config.state_path)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?,
    );
    let service: Arc<dyn SignerService> = Arc::new(DefaultSignerService::with_registry(
        Arc::new(registry),
        store,
        Arc::new(policy),
    ));
    let runtime = Arc::new(NodeSignerRuntime {
        service,
        default_provider: default.id.clone(),
        policy_version: config.policy_version.clone(),
        request_timeout: Duration::from_millis(config.request_timeout_ms),
        key_resolver,
        purposes,
    });
    info!(
        target: "neo::signing",
        default_provider = %runtime.default_provider,
        providers = enabled.len(),
        security_level = ?default.security_level,
        policy_version = %runtime.policy_version,
        "unified signer service initialized"
    );
    Ok(Some(runtime))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SignerKeySection, SignerProviderSection};
    use neo_crypto::Secp256r1Crypto;
    use neo_protocol::ProtocolSettings;
    use neo_wallets::signing::{IdempotencyInfo, ProtocolContext, SignRequest};
    use neo_wallets::{KeyPair, Nep6Wallet};

    fn service_section(provider: SignerProviderSection, state_path: &str) -> SignerServiceSection {
        SignerServiceSection {
            enabled: true,
            state_path: state_path.to_string(),
            policy_version: "policy-v1".to_string(),
            request_timeout_ms: 5_000,
            max_fee_units: 100,
            quota_limit_units: 100,
            quota_window_ms: 60_000,
            providers: vec![provider],
            required_purposes: vec![SignPurpose::Transaction],
            required_canonical_encodings: vec![NEO_SIGN_DATA_ENCODING.to_string()],
            required_algorithms: vec![ALGORITHM_ECDSA_P256_SHA256.to_string()],
            required_security_level: Some(SecurityLevel::Software),
        }
    }

    fn provider_section(public_key: &[u8], account: &str) -> SignerProviderSection {
        SignerProviderSection {
            id: "wallet".to_string(),
            kind: SignerProviderKind::Nep6,
            enabled: true,
            default: true,
            security_level: Some(SecurityLevel::Software),
            purposes: vec![SignPurpose::Transaction],
            canonical_encodings: vec![NEO_SIGN_DATA_ENCODING.to_string()],
            algorithms: vec![ALGORITHM_ECDSA_P256_SHA256.to_string()],
            keys: vec![SignerKeySection {
                key_id: "transaction-key".to_string(),
                version: 2,
                public_key_hex: hex::encode(public_key),
                account: account.to_string(),
                purposes: vec![SignPurpose::Transaction],
            }],
            endpoint: None,
            remote: None,
            pkcs11: None,
        }
    }

    fn signing_request(account: &str) -> SignRequest {
        SignRequest {
            schema_version: SIGNING_SCHEMA_VERSION,
            request_id: "startup-transaction-1".to_string(),
            network: 860_833_102,
            purpose: SignPurpose::Transaction,
            key_ref: KeyRef::new("wallet", "transaction-key", 2),
            payload: SignPayload::Canonical {
                encoding: NEO_SIGN_DATA_ENCODING.to_string(),
                bytes: vec![1, 2, 3, 4, 5],
            },
            protocol_context: ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: "neo-n3-transaction".to_string(),
                protocol_version: "1".to_string(),
                fields: BTreeMap::new(),
            },
            policy_version: "policy-v1".to_string(),
            deadline_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64
                + 60_000,
            idempotency: IdempotencyInfo {
                schema_version: SIGNING_SCHEMA_VERSION,
                key: "startup-transaction-idempotency".to_string(),
                replay_nonce: "startup-transaction-nonce".to_string(),
            },
            fee_units: 1,
            quota_units: 1,
            account: Some(account.to_string()),
            targets: BTreeSet::new(),
        }
    }

    #[tokio::test]
    async fn local_wallet_runtime_signs_through_policy_and_durable_store() {
        let settings = Arc::new(ProtocolSettings::mainnet());
        let wallet = Arc::new(Nep6Wallet::create(
            Some("startup".to_string()),
            None,
            settings,
        ));
        let key_pair = KeyPair::from_private_key(&[0x11; 32]).expect("key");
        let public_key = key_pair.public_key();
        let contract = neo_wallets::Contract::create_signature_contract(
            key_pair.get_public_key_point().expect("public key point"),
        );
        let account = wallet
            .create_account_with_contract(contract, Some(key_pair))
            .expect("account");
        let account_hash = account.script_hash().to_string();
        let provider = NodeWalletProvider::new();
        provider.set_wallet(Some(wallet));
        let directory = tempfile::tempdir().expect("tempdir");
        let state_path = directory.path().join("signing.journal");
        let config = service_section(
            provider_section(&public_key, &account_hash),
            &state_path.to_string_lossy(),
        );
        let runtime = initialize_signer_service(
            Some(&config),
            860_833_102,
            Some(&Arc::new(provider)),
            Some(ActiveSignerBackend {
                kind: SignerProviderKind::Nep6,
                security_level: SecurityLevel::Software,
            }),
        )
        .expect("initialize")
        .expect("runtime");
        let client = runtime
            .trusted_client(RPC_SIGNING_PRINCIPAL, "node-local-test", "sign:transaction")
            .expect("trusted client");
        let response = client
            .sign(signing_request(&account_hash))
            .await
            .expect("authorized signature");
        assert_eq!(response.signature.len(), 64);
        assert!(
            Secp256r1Crypto::verify(
                &[1, 2, 3, 4, 5],
                response
                    .signature
                    .as_slice()
                    .try_into()
                    .expect("64-byte signature"),
                &public_key,
            )
            .expect("verification")
        );
    }

    #[test]
    fn hardware_tee_never_falls_back_to_a_different_security_boundary() {
        let mut provider = provider_section(&[2; 33], &UInt160::zero().to_string());
        provider.kind = SignerProviderKind::HardwareTee;
        provider.security_level = Some(SecurityLevel::HardwareTee);
        let directory = tempfile::tempdir().expect("tempdir");
        let mut config = service_section(
            provider,
            &directory.path().join("journal").to_string_lossy(),
        );
        config.required_purposes.clear();
        config.required_security_level = None;
        let error = initialize_signer_service(Some(&config), 860_833_102, None, None)
            .expect_err("unavailable attested hardware must fail");
        assert!(error.to_string().contains("refusing to substitute"));
    }

    #[test]
    fn disabled_signer_service_has_no_runtime_side_effects() {
        let config = SignerServiceSection::default();
        let runtime = initialize_signer_service(Some(&config), 1, None, None).expect("disabled");
        assert!(runtime.is_none());
    }
}
