//! Unified signer providers and node-local signing runtime.

use crate::config::{SignerProviderKind, SignerServiceSection};
use crate::pkcs11::{HsmSigner, LibPkcs11Backend, Pkcs11Backend};
use crate::remote_signer::{MtlsRemoteSignerTransport, RemoteSignerProvider};
use crate::wallet_provider::NodeWalletProvider;
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use neo_wallets::WalletProvider;
use neo_primitives::UInt160;
use neo_signing::{
    ALGORITHM_ECDSA_P256_SHA256, AuthenticatedContext, AuthorizedSignRequest, DefaultSignerService,
    DurableSigningStore, KeyInfo, KeyRef, PolicyLimits, ProviderCapabilities,
    ProviderOutcomeProvenance, ProviderSignature, SIGNING_SCHEMA_VERSION, SecurityLevel, SignError,
    SignErrorCode, SignPayload, SignPurpose, SignerProvider, SignerProviderRegistry, SignerService,
    SigningKeyBinding, SigningKeyResolver, SigningPolicy, SigningStore, TrustedSignerClient,
};
use neo_smart_contract::helper::Helper as ContractHelper;
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

pub(crate) const NEO_SIGN_DATA_ENCODING: &str = "neo-n3-sign-data-v1";
pub(crate) const RPC_SIGNING_PRINCIPAL: &str = "neo-rpc-wallet";
pub(crate) const CONSENSUS_SIGNING_PRINCIPAL: &str = "neo-consensus";
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

    pub(crate) fn resolve_key(
        &self,
        account: &str,
        purpose: &SignPurpose,
    ) -> neo_signing::SignResult<KeyRef> {
        self.key_resolver.resolve(account, purpose)
    }

    pub(crate) fn key_resolver(&self) -> Arc<SigningKeyResolver> {
        Arc::clone(&self.key_resolver)
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        service: Arc<dyn SignerService>,
        default_provider: impl Into<String>,
        policy_version: impl Into<String>,
        request_timeout: Duration,
        key_resolver: Arc<SigningKeyResolver>,
    ) -> Self {
        Self {
            service,
            default_provider: default_provider.into(),
            policy_version: policy_version.into(),
            request_timeout,
            key_resolver,
        }
    }

    pub(crate) fn trusted_client(
        &self,
        principal: &str,
        authentication_method: &str,
        scope: &str,
    ) -> neo_signing::SignResult<TrustedSignerClient> {
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
    wallet: Arc<dyn neo_core::wallets::Wallet>,
    capabilities: ProviderCapabilities,
    keys: BTreeMap<KeyRef, (UInt160, Vec<u8>)>,
}

impl WalletSignerProvider {
    fn new(
        wallet: Arc<dyn neo_core::wallets::Wallet>,
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
                    .expect("validated signer provider security level"),
                outcome_provenance: ProviderOutcomeProvenance::DeterministicLocal,
            },
        })
    }

    fn configured_key(&self, key_ref: &KeyRef) -> neo_signing::SignResult<(UInt160, Vec<u8>)> {
        self.keys.get(key_ref).cloned().ok_or_else(|| {
            SignError::new(
                SignErrorCode::KeyNotFound,
                "signer key id and version are not configured for the wallet provider",
            )
        })
    }

    fn wallet(&self) -> Arc<dyn neo_core::wallets::Wallet> {
        Arc::clone(&self.wallet)
    }

    fn public_key_for(
        &self,
        key_ref: &KeyRef,
    ) -> neo_signing::SignResult<(Arc<dyn neo_core::wallets::Wallet>, UInt160, Vec<u8>)> {
        let (script_hash, configured_public_key) = self.configured_key(key_ref)?;
        let wallet = self.wallet();
        let account = wallet.get_account(&script_hash).ok_or_else(|| {
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
        Ok((wallet, script_hash, wallet_public_key))
    }
}

#[async_trait]
impl SignerProvider for WalletSignerProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }

    async fn key_info(&self, key_ref: &KeyRef) -> neo_signing::SignResult<KeyInfo> {
        let (wallet, _, public_key) = self.public_key_for(key_ref)?;
        Ok(KeyInfo {
            schema_version: SIGNING_SCHEMA_VERSION,
            key_ref: key_ref.clone(),
            algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
            public_key,
            security_level: self.capabilities.security_level,
            enabled: true,
            attributes: BTreeMap::from([("wallet".to_string(), wallet.name().to_string())]),
        })
    }

    async fn sign_authorized(
        &self,
        request: &AuthorizedSignRequest,
    ) -> neo_signing::SignResult<ProviderSignature> {
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
        let (wallet, script_hash, _) = self.public_key_for(request.key_ref())?;
        let signature = wallet.sign(bytes, &script_hash).await.map_err(|error| {
            SignError::new(
                SignErrorCode::ProviderFailure,
                format!("wallet signing failed: {error}"),
            )
        })?;
        Ok(ProviderSignature {
            algorithm: ALGORITHM_ECDSA_P256_SHA256.to_string(),
            signature,
        })
    }
}

pub(crate) fn initialize_signer_service(
    config: Option<&SignerServiceSection>,
    network: u32,
    wallet_provider: Option<&Arc<NodeWalletProvider>>,
    active_backend: Option<ActiveSignerBackend>,
) -> Result<Option<Arc<NodeSignerRuntime>>> {
    initialize_signer_service_with_hardware_tee(
        config,
        network,
        wallet_provider,
        active_backend,
        default_hardware_tee_signer,
    )
}

/// Production factory: loads the configured PKCS#11 module and fails closed.
fn default_hardware_tee_signer(
    provider: &crate::config::SignerProviderSection,
) -> Result<Arc<dyn SignerProvider>> {
    let backend: Arc<dyn Pkcs11Backend> = Arc::new(
        LibPkcs11Backend::from_provider_section(provider).with_context(|| {
            format!(
                "failed to load the PKCS#11 backend for signer provider '{}'",
                provider.id
            )
        })?,
    );
    Ok(Arc::new(HsmSigner::from_env_pin(provider, backend)?))
}

fn initialize_signer_service_with_hardware_tee<F>(
    config: Option<&SignerServiceSection>,
    network: u32,
    wallet_provider: Option<&Arc<NodeWalletProvider>>,
    active_backend: Option<ActiveSignerBackend>,
    hardware_tee_factory: F,
) -> Result<Option<Arc<NodeSignerRuntime>>>
where
    F: Fn(&crate::config::SignerProviderSection) -> Result<Arc<dyn SignerProvider>>,
{
    let Some(config) = config.filter(|config| config.enabled) else {
        return Ok(None);
    };
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
    let signable_accounts = wallet
        .as_ref()
        .map(|wallet| {
            wallet
                .get_accounts()
                .into_iter()
                .filter(|account| !account.is_locked() && account.has_key())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if has_local && signable_accounts.is_empty() {
        bail!("signer service wallet has no unlocked signable accounts");
    }

    let mut registry = SignerProviderRegistry::new();
    for provider in &enabled {
        let registered: Arc<dyn SignerProvider> = match provider.kind {
            SignerProviderKind::Remote | SignerProviderKind::RemoteTee => {
                let transport: Arc<dyn neo_signing::RemoteSignerTransport> = Arc::new(
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
                // The hardware TEE provider owns its keys inside a PKCS#11
                // token: it neither requires the node wallet nor an active
                // wallet-backed backend, and construction fails closed.
                hardware_tee_factory(provider).with_context(|| {
                    format!(
                        "failed to construct the hardware TEE signer provider '{}'",
                        provider.id
                    )
                })?
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
                    Arc::clone(wallet.as_ref().expect("validated local wallet")),
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
            let public_key = hex::decode(&key.public_key_hex).with_context(|| {
                format!(
                    "signer provider '{}' key '{}' has invalid public key",
                    provider.id, key.key_id
                )
            })?;
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
        CONSENSUS_SIGNING_PRINCIPAL.to_string(),
        STATE_SIGNING_PRINCIPAL.to_string(),
        ORACLE_SIGNING_PRINCIPAL.to_string(),
    ]);
    policy.tenants.allow.insert(NODE_SIGNING_TENANT.to_string());
    policy.networks.allow.insert(network);
    for provider in &enabled {
        let level = provider.security_level.expect("validated security level");
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
                let principal = match purpose {
                    SignPurpose::Transaction => RPC_SIGNING_PRINCIPAL,
                    SignPurpose::Consensus => CONSENSUS_SIGNING_PRINCIPAL,
                    SignPurpose::StateRoot => STATE_SIGNING_PRINCIPAL,
                    SignPurpose::OracleResponse => ORACLE_SIGNING_PRINCIPAL,
                    _ => continue,
                };
                let scope = purpose.required_scope();
                policy
                    .authorizations
                    .allow
                    .insert(SigningPolicy::authorization_selector(
                        principal,
                        NODE_SIGNING_TENANT,
                        &key_ref,
                        network,
                        purpose,
                        &scope,
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
    use crate::config::{Pkcs11ProviderSection, SignerKeySection, SignerProviderSection};
    use crate::pkcs11::Pkcs11Mechanism;
    use crate::pkcs11::mock::MockP11Backend;
    use neo_wallets::{KeyPair, Nep6Wallet, Wallet};
    use neo_protocol::ProtocolSettings;

    struct UnusedSignerService;

    #[async_trait]
    impl SignerService for UnusedSignerService {
        async fn sign(
            &self,
            _request: neo_signing::SignRequest,
            _context: AuthenticatedContext,
        ) -> neo_signing::SignResult<neo_signing::SignResponse> {
            Err(SignError::new(
                SignErrorCode::ProviderFailure,
                "unused test signer service",
            ))
        }

        async fn resume_or_start(
            &self,
            _operation_id: &str,
            _frozen_intent: neo_signing::FrozenSignIntent,
            _context: AuthenticatedContext,
            _request_timeout: Duration,
        ) -> neo_signing::SignResult<neo_signing::SignResponse> {
            Err(SignError::new(
                SignErrorCode::ProviderFailure,
                "unused test signer service",
            ))
        }
    }

    #[tokio::test]
    async fn wallet_signer_provider_remains_bound_to_startup_wallet() {
        let settings = Arc::new(ProtocolSettings::default());
        let startup_wallet = Arc::new(Nep6Wallet::new(
            Some("startup".to_string()),
            None,
            Arc::clone(&settings),
        ));
        let startup_account = startup_wallet
            .create_account(&[0x11; 32])
            .await
            .expect("create startup account");
        let startup_wallet_trait: Arc<dyn Wallet> = startup_wallet;

        let configured_key = KeyPair::from_private_key(&[0x11; 32]).expect("configured key");
        let provider_config = SignerProviderSection {
            id: "wallet".to_string(),
            kind: SignerProviderKind::Nep6,
            enabled: true,
            default: true,
            security_level: Some(SecurityLevel::Software),
            purposes: vec![neo_signing::SignPurpose::Transaction],
            canonical_encodings: vec![NEO_SIGN_DATA_ENCODING.to_string()],
            algorithms: vec![ALGORITHM_ECDSA_P256_SHA256.to_string()],
            keys: vec![SignerKeySection {
                key_id: "startup-key".to_string(),
                version: 2,
                public_key_hex: hex::encode(configured_key.public_key()),
                account: startup_account.script_hash().to_string(),
                purposes: vec![SignPurpose::Transaction],
            }],
            endpoint: None,
            remote: None,
            pkcs11: None,
        };
        let signer = WalletSignerProvider::new(Arc::clone(&startup_wallet_trait), &provider_config)
            .expect("configured wallet signer");

        let replaceable_provider = NodeWalletProvider::new();
        replaceable_provider.set_wallet(Some(startup_wallet_trait));
        let replacement_wallet: Arc<dyn Wallet> = Arc::new(Nep6Wallet::new(
            Some("replacement".to_string()),
            None,
            settings,
        ));
        replaceable_provider.set_wallet(Some(replacement_wallet));

        let info = signer
            .key_info(&KeyRef::new("wallet", "startup-key", 2))
            .await
            .expect("captured startup wallet remains available");
        assert_eq!(
            info.attributes.get("wallet").map(String::as_str),
            Some("startup")
        );
    }

    const HARDWARE_TEE_TEST_NETWORK: u32 = 860_833_102;

    fn hardware_tee_service_section(provider: SignerProviderSection) -> SignerServiceSection {
        SignerServiceSection {
            enabled: true,
            state_path: "data/unused-hsm-signing.journal".to_string(),
            policy_version: "policy-v1".to_string(),
            request_timeout_ms: 5_000,
            max_fee_units: 100,
            quota_limit_units: 100,
            quota_window_ms: 60_000,
            providers: vec![provider],
            required_purposes: Vec::new(),
            required_canonical_encodings: Vec::new(),
            required_algorithms: Vec::new(),
            required_security_level: None,
        }
    }

    fn hardware_tee_provider_section(public_key_hex: &str, account: &str) -> SignerProviderSection {
        SignerProviderSection {
            id: "hsm-tee".to_string(),
            kind: SignerProviderKind::HardwareTee,
            enabled: true,
            default: true,
            security_level: Some(SecurityLevel::HardwareTee),
            purposes: vec![SignPurpose::Consensus],
            canonical_encodings: vec![NEO_SIGN_DATA_ENCODING.to_string()],
            algorithms: vec![ALGORITHM_ECDSA_P256_SHA256.to_string()],
            keys: vec![SignerKeySection {
                key_id: "validator-key".to_string(),
                version: 2,
                public_key_hex: public_key_hex.to_string(),
                account: account.to_string(),
                purposes: vec![SignPurpose::Consensus],
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

    fn consensus_request(
        request_id: &str,
        key_ref: KeyRef,
        account: &str,
    ) -> neo_signing::SignRequest {
        neo_signing::SignRequest {
            schema_version: SIGNING_SCHEMA_VERSION,
            request_id: request_id.to_string(),
            network: HARDWARE_TEE_TEST_NETWORK,
            purpose: SignPurpose::Consensus,
            key_ref,
            payload: neo_signing::SignPayload::Canonical {
                encoding: NEO_SIGN_DATA_ENCODING.to_string(),
                bytes: vec![1, 2, 3, 4, 5],
            },
            protocol_context: neo_signing::ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: "neo-n3-consensus".to_string(),
                protocol_version: "1".to_string(),
                fields: BTreeMap::new(),
            },
            policy_version: "policy-v1".to_string(),
            deadline_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64
                + 60_000,
            idempotency: neo_signing::IdempotencyInfo {
                schema_version: SIGNING_SCHEMA_VERSION,
                key: format!("idem-{request_id}"),
                replay_nonce: format!("nonce-{request_id}"),
            },
            fee_units: 1,
            quota_units: 1,
            account: Some(account.to_string()),
            targets: BTreeSet::new(),
        }
    }

    fn consensus_context() -> AuthenticatedContext {
        AuthenticatedContext {
            schema_version: SIGNING_SCHEMA_VERSION,
            principal: CONSENSUS_SIGNING_PRINCIPAL.to_string(),
            tenant: NODE_SIGNING_TENANT.to_string(),
            authentication_method: "node-local-test".to_string(),
            scopes: BTreeSet::from(["sign:consensus".to_string()]),
            claims: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn hardware_tee_signer_provider_constructs_without_wallet_or_active_backend() {
        let private_key = [0x53; 32];
        let backend = MockP11Backend::new(0, "TESTTOKEN", "validator-key", &private_key, "4321");
        let public_key = backend.public_key().to_vec();
        let script_hash = ContractHelper::signature_redeem_script(&public_key);
        let account =
            neo_primitives::UInt160::from(neo_crypto::Crypto::hash160(&script_hash)).to_string();
        let section = hardware_tee_provider_section(&hex::encode(&public_key), &account);
        let mock: Arc<dyn Pkcs11Backend> = Arc::new(backend.clone());
        let directory = tempfile::tempdir().expect("temporary signing store");
        let mut service_section = hardware_tee_service_section(section);
        service_section.state_path = directory
            .path()
            .join("signing.journal")
            .to_string_lossy()
            .into_owned();

        // No wallet provider and no active wallet-backed backend: the PKCS#11
        // adapter replaces the historical startup bail.
        let runtime = initialize_signer_service_with_hardware_tee(
            Some(&service_section),
            HARDWARE_TEE_TEST_NETWORK,
            None,
            None,
            |provider: &crate::config::SignerProviderSection| {
                Ok(Arc::new(HsmSigner::new(
                    provider,
                    Arc::clone(&mock),
                    zeroize::Zeroizing::new("4321".as_bytes().to_vec()),
                )?))
            },
        )
        .expect("hardware TEE signer service constructs without a wallet")
        .expect("enabled signer service returns a runtime");

        let request = consensus_request(
            "hardware-tee-consensus-1",
            KeyRef::new("hsm-tee", "validator-key", 2),
            &account,
        );
        let response = runtime
            .service()
            .sign(request, consensus_context())
            .await
            .expect("authorized consensus signing through the PKCS#11 adapter");
        assert_eq!(response.signature.len(), 64);
        assert_eq!(backend.recorded_signs().len(), 1);
        assert_eq!(
            backend.recorded_signs()[0].mechanism,
            Pkcs11Mechanism::EcdsaSha256
        );
    }

    #[test]
    fn hardware_tee_signer_service_fails_closed_on_invalid_library_path() {
        let private_key = [0x53; 32];
        let backend = MockP11Backend::new(0, "TESTTOKEN", "validator-key", &private_key, "4321");
        let public_key = backend.public_key().to_vec();
        let script_hash = ContractHelper::signature_redeem_script(&public_key);
        let account =
            neo_primitives::UInt160::from(neo_crypto::Crypto::hash160(&script_hash)).to_string();
        let mut section = hardware_tee_provider_section(&hex::encode(&public_key), &account);
        section
            .pkcs11
            .as_mut()
            .expect("pkcs11 section")
            .library_path = "Z:\\definitely\\missing\\pkcs11-module.dll".to_string();
        let service_section = hardware_tee_service_section(section);

        // The production factory must fail closed on an unresolvable PKCS#11
        // library instead of silently degrading to a software signer.
        let error = initialize_signer_service(
            Some(&service_section),
            HARDWARE_TEE_TEST_NETWORK,
            None,
            None,
        )
        .expect_err("invalid PKCS#11 library path must fail closed");
        let chain = format!("{error:#}");
        assert!(chain.contains("PKCS#11"), "unexpected error: {chain}");
    }
}
