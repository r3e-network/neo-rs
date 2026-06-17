//! Node-level dBFT consensus driver.
//!
//! Wires the verified `neo-consensus` `ConsensusService` state machine into a
//! running node: it instantiates the service from the validator configuration,
//! converts the service's outbound `ConsensusEvent`s into network/mempool/ledger
//! actions, decodes inbound dBFT `ExtensiblePayload`s from peers back into
//! `ConsensusPayload`s, and drives the per-block round lifecycle.
//!
//! The block-production primitives this builds on (the dBFT state machine,
//! `BlockData::assemble_block`) are verified in neo-consensus; the end-to-end
//! multi-node consensus behaviour is exercised only in a real deployment.
//!
//! Compiled as part of the default daemon feature set.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use neo_blockchain::{BlockchainCommand, BlockchainHandle, RuntimeEvent};
use neo_config::ProtocolSettings;
use neo_consensus::messages::{ConsensusPayload, PrepareRequestMessage};
use neo_consensus::{
    ChangeViewReason, ConsensusEvent, ConsensusMessageType, ConsensusService, ConsensusSigner,
    ValidatorInfo,
};
use neo_crypto::{ECPoint, Secp256r1Crypto};
use neo_io::{Serializable, serializable::helper::SerializeHelper};
use neo_mempool::{MemoryPool, PoolItem, verify_transaction};
use neo_native_contracts::{LedgerContract, NeoToken, PolicyContract};
use neo_network::NetworkHandle;
use neo_payloads::{ExtensiblePayload, Transaction, TransactionAttribute, Witness};
use neo_primitives::{UInt160, UInt256, VerifyResult};
use neo_storage::persistence::DataCache;
use neo_vm::script_builder::RedeemScript;
use num_bigint::BigInt;
use parking_lot::RwLock;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// dBFT extensible category (C# `ConsensusContext.CreatePayload`: `Category = "dBFT"`).
const DBFT_CATEGORY: &str = "dBFT";
/// Block version dBFT produces (C# Header default; consensus never sets a non-zero version).
const BLOCK_VERSION: u32 = 0;
/// C# DBFTPlugin `DbftSettings.MaxBlockSystemFee` default for Neo v3.10.0.
const DBFT_MAX_BLOCK_SYSTEM_FEE: i64 = 150_000_000_000;

/// Milliseconds since the Unix epoch — the same clock the consensus crate uses
/// for view-timeout accounting.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ===================== Extensible <-> ConsensusPayload codec =====================

/// `PUSHDATA1 0x40 <64-byte sig>` — a single-signature invocation script.
fn invocation_script_from_signature(signature: &[u8]) -> Vec<u8> {
    let mut script = Vec::with_capacity(2 + signature.len());
    script.push(neo_vm_rs::OpCode::PUSHDATA1.byte());
    script.push(signature.len() as u8);
    script.extend_from_slice(signature);
    script
}

/// Extracts the raw 64-byte signature from a single-sig invocation script.
fn signature_from_invocation_script(invocation: &[u8]) -> Option<&[u8]> {
    if invocation.len() != 66
        || invocation[0] != neo_vm_rs::OpCode::PUSHDATA1.byte()
        || invocation[1] != 0x40
    {
        return None;
    }
    Some(&invocation[2..66])
}

/// Builds the outbound dBFT [`ExtensiblePayload`] for a `ConsensusPayload` the
/// service produced (its `witness` is the raw 64-byte signature). Mirrors C#
/// `ConsensusContext.CreatePayload`.
fn consensus_to_extensible(
    payload: &ConsensusPayload,
    validators: &[ValidatorInfo],
) -> Option<ExtensiblePayload> {
    let validator = validators.get(payload.validator_index as usize)?;
    let mut ext = ExtensiblePayload::new();
    ext.category = DBFT_CATEGORY.to_string();
    ext.valid_block_start = 0;
    ext.valid_block_end = payload.block_index;
    ext.sender = validator.script_hash;
    ext.data = payload.to_message_bytes();
    ext.witness = Witness::new_with_scripts(
        invocation_script_from_signature(&payload.witness),
        RedeemScript::signature_redeem_script(&validator.public_key.encoded()),
    );
    Some(ext)
}

/// Decodes an inbound dBFT [`ExtensiblePayload`] into a [`ConsensusPayload`].
/// Returns `None` for non-dBFT, malformed, or spoofed payloads (the in-body
/// `validator_index` must map to the validator whose script hash is the
/// extensible's `sender`).
pub fn extensible_to_consensus(
    ext: &ExtensiblePayload,
    network: u32,
    validators: &[ValidatorInfo],
) -> Option<ConsensusPayload> {
    if ext.category != DBFT_CATEGORY {
        return None;
    }
    let signature = signature_from_invocation_script(&ext.witness.invocation_script)?;
    let payload =
        ConsensusPayload::from_message_bytes(network, &ext.data, signature.to_vec()).ok()?;
    let validator = validators.get(payload.validator_index as usize)?;
    if validator.script_hash != ext.sender {
        return None;
    }
    Some(payload)
}

// ===================== validator-set + key derivation =====================

fn validator_infos_from_keys(keys: Vec<ECPoint>) -> Vec<ValidatorInfo> {
    keys.into_iter()
        .enumerate()
        .map(|(index, public_key)| {
            let script_hash = UInt160::from_script(&RedeemScript::signature_redeem_script(
                public_key.as_bytes(),
            ));
            ValidatorInfo {
                index: index as u8,
                public_key,
                script_hash,
            }
        })
        .collect()
}

/// Builds the ordered dBFT validator set from the protocol settings.
///
/// C# dBFT uses `NEO.GetNextBlockValidators(...).OrderBy(p => p)`, which at
/// genesis reduces to `StandbyCommittee.Take(ValidatorsCount).OrderBy(p => p)`.
/// `standby_validators()` does the `Take(N)` but NOT the sort; the validator
/// **index** (and thus primary selection) depends on the sorted order, so the
/// keys are sorted here.
pub fn build_consensus_validators(settings: &ProtocolSettings) -> Vec<ValidatorInfo> {
    let mut keys: Vec<ECPoint> = settings.standby_validators();
    keys.sort();
    validator_infos_from_keys(keys)
}

/// Finds this node's validator index by deriving its public key from the
/// private key and matching it against the (sorted) validator set. `None` when
/// this node is not a consensus validator (it then only relays).
pub fn resolve_my_index(private_key: &[u8; 32], validators: &[ValidatorInfo]) -> Option<u8> {
    let pub_bytes = Secp256r1Crypto::derive_public_key(private_key).ok()?;
    let my_pubkey = ECPoint::from_bytes(&pub_bytes).ok()?;
    resolve_public_key_index(&my_pubkey, validators)
}

fn resolve_public_key_index(public_key: &ECPoint, validators: &[ValidatorInfo]) -> Option<u8> {
    validators
        .iter()
        .find(|v| &v.public_key == public_key)
        .map(|v| v.index)
}

/// Resolved consensus configuration: the validator set and this node's key/index.
pub struct ConsensusSetup {
    /// The ordered dBFT validator set.
    pub validators: Vec<ValidatorInfo>,
    /// This node's public key, derived from `private_key`.
    pub public_key: ECPoint,
    /// This node's validator index, or `None` (observer; relay-only).
    pub my_index: Option<u8>,
    /// This node's 32-byte secp256r1 software private key. Zeroed and unused
    /// when `signer` is set (HSM-backed signing).
    pub private_key: [u8; 32],
    /// Network magic.
    pub network: u32,
    /// Target block time (ms) — the view-timeout base.
    pub ms_per_block: u64,
    /// Optional HSM-backed signer. When `Some`, the consensus service signs via
    /// this signer (keyed by the validator script hash) instead of `private_key`.
    pub signer: Option<Arc<dyn ConsensusSigner>>,
}

/// `[consensus.hsm]`: HSM-backed consensus signing over PKCS#11.
///
/// The PIN is never stored in the TOML; it is read at startup from the
/// `pin_env` environment variable (default `NEO_HSM_CU_PASSWORD`). Requires
/// the node to be built with `--features hsm`.
#[derive(Debug, Clone, Deserialize)]
pub struct HsmKeyConfig {
    /// `aws` | `azure-cloud-hsm` | `azure-dedicated-hsm` | `gcp-cloud-hsm` | `generic`.
    pub provider: String,
    /// PKCS#11 `.so` to load; defaults to the provider's library when omitted.
    #[serde(default)]
    pub library_path: Option<String>,
    /// PKCS#11 slot number; first slot with a token present when omitted.
    #[serde(default)]
    pub slot: Option<u64>,
    /// Token label to match when `slot` is omitted.
    #[serde(default)]
    pub token_label: Option<String>,
    /// `CKA_LABEL` of the consensus private key.
    pub key_label: String,
    /// Optional `CKA_ID` (hex) to disambiguate keys sharing a label.
    #[serde(default)]
    pub key_id_hex: Option<String>,
    /// Environment variable holding the `C_Login` PIN.
    #[serde(default = "default_hsm_pin_env")]
    pub pin_env: String,
}

fn default_hsm_pin_env() -> String {
    "NEO_HSM_CU_PASSWORD".to_string()
}

/// Builds the consensus setup from the protocol settings and the `[consensus]`
/// configuration. Returns `Ok(None)` when consensus is disabled. Returns an
/// error when consensus is enabled but the validator key is missing/malformed.
pub fn build_consensus_setup(
    settings: &ProtocolSettings,
    enabled: bool,
    private_key_hex: Option<&str>,
    hsm: Option<&HsmKeyConfig>,
) -> anyhow::Result<Option<ConsensusSetup>> {
    if !enabled {
        return Ok(None);
    }

    let validators = build_consensus_validators(settings);

    // HSM-backed signing takes precedence over a software key when configured.
    if let Some(hsm_cfg) = hsm {
        if private_key_hex.is_some() {
            warn!("[consensus] both private_key_hex and [consensus.hsm] are set; using the HSM");
        }
        return build_hsm_consensus_setup(settings, validators, hsm_cfg);
    }

    let hex_key = private_key_hex.ok_or_else(|| {
        anyhow::anyhow!(
            "[consensus].enabled = true requires [consensus].private_key_hex or [consensus.hsm]"
        )
    })?;
    let raw = hex::decode(hex_key.trim())
        .map_err(|e| anyhow::anyhow!("invalid [consensus].private_key_hex: {e}"))?;
    let private_key: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("[consensus].private_key_hex must be 32 bytes"))?;
    let public_key = ECPoint::from_bytes(
        &Secp256r1Crypto::derive_public_key(&private_key)
            .map_err(|e| anyhow::anyhow!("failed to derive consensus public key: {e}"))?,
    )
    .map_err(|e| anyhow::anyhow!("failed to decode consensus public key: {e}"))?;

    let my_index = resolve_my_index(&private_key, &validators);
    Ok(Some(ConsensusSetup {
        validators,
        public_key,
        my_index,
        private_key,
        network: settings.network,
        ms_per_block: u64::from(settings.milliseconds_per_block),
        signer: None,
    }))
}

/// Connects to the configured HSM, derives this node's validator index from the
/// HSM public key, and returns a setup whose signing is HSM-backed (the software
/// `private_key` is left zeroed and unused).
#[cfg(feature = "hsm")]
fn build_hsm_consensus_setup(
    settings: &ProtocolSettings,
    validators: Vec<ValidatorInfo>,
    cfg: &HsmKeyConfig,
) -> anyhow::Result<Option<ConsensusSetup>> {
    use std::path::PathBuf;

    let provider = match cfg.provider.to_ascii_lowercase().as_str() {
        "aws" => neo_hsm::HsmProvider::Aws,
        "azure-cloud-hsm" | "azure" => neo_hsm::HsmProvider::AzureCloudHsm,
        "azure-dedicated-hsm" => neo_hsm::HsmProvider::AzureDedicatedHsm,
        "gcp-cloud-hsm" | "gcp" => neo_hsm::HsmProvider::GcpCloudHsm,
        "yubihsm2" | "yubihsm" => neo_hsm::HsmProvider::YubiHsm2,
        "nshield" => neo_hsm::HsmProvider::NShield,
        "softhsm2" | "softhsm" => neo_hsm::HsmProvider::SoftHsm2,
        "utimaco" => neo_hsm::HsmProvider::Utimaco,
        "generic" | "generic-pkcs11" => neo_hsm::HsmProvider::GenericPkcs11,
        other => anyhow::bail!("[consensus.hsm].provider {other:?} is not recognized"),
    };
    let library_path = cfg
        .library_path
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(neo_hsm::profile(provider).default_library));
    let key_id = match &cfg.key_id_hex {
        Some(h) => Some(
            hex::decode(h.trim())
                .map_err(|e| anyhow::anyhow!("[consensus.hsm].key_id_hex is not valid hex: {e}"))?,
        ),
        None => None,
    };
    let user_pin = std::env::var(&cfg.pin_env)
        .map_err(|_| anyhow::anyhow!("[consensus.hsm] PIN env var {} is not set", cfg.pin_env))?;

    let hsm_config = neo_hsm::HsmConfig {
        provider,
        library_path,
        slot: cfg.slot,
        token_label: cfg.token_label.clone(),
        key_label: cfg.key_label.clone(),
        key_id,
        user_pin,
    };
    let signer = neo_hsm::Pkcs11Signer::connect(&hsm_config)
        .map_err(|e| anyhow::anyhow!("HSM connect failed: {e}"))?;
    let public_key = ECPoint::from_bytes(signer.public_key())
        .map_err(|e| anyhow::anyhow!("HSM public key is not a valid secp256r1 point: {e}"))?;
    let my_index = resolve_public_key_index(&public_key, &validators);
    if my_index.is_none() {
        warn!(
            pubkey = %hex::encode(signer.public_key()),
            "HSM consensus key is not in the validator set; the node will relay only"
        );
    }
    info!(provider = ?cfg.provider, "HSM consensus signer connected");
    Ok(Some(ConsensusSetup {
        validators,
        public_key,
        my_index,
        private_key: [0u8; 32],
        network: settings.network,
        ms_per_block: u64::from(settings.milliseconds_per_block),
        signer: Some(Arc::new(signer)),
    }))
}

#[cfg(not(feature = "hsm"))]
fn build_hsm_consensus_setup(
    _settings: &ProtocolSettings,
    _validators: Vec<ValidatorInfo>,
    _cfg: &HsmKeyConfig,
) -> anyhow::Result<Option<ConsensusSetup>> {
    anyhow::bail!(
        "[consensus.hsm] is configured but neo-node was built without the `hsm` feature; rebuild with --features hsm"
    )
}

// ===================== the driver =====================

/// Resolves the full transactions for `hashes`, in block order, from the
/// proposal cache then the live mempool. Returns `None` if any is missing.
fn resolve_transactions(
    hashes: &[UInt256],
    cache: &HashMap<UInt256, Arc<Transaction>>,
    mempool: &MemoryPool,
) -> Option<Vec<Transaction>> {
    let mut out = Vec::with_capacity(hashes.len());
    for hash in hashes {
        if let Some(tx) = cache.get(hash) {
            out.push((**tx).clone());
        } else if let Some(item) = mempool.get(hash) {
            out.push((*item.transaction).clone());
        } else {
            return None;
        }
    }
    Some(out)
}

#[derive(Default)]
struct ProposalVerificationContext {
    transactions: HashMap<UInt256, Arc<Transaction>>,
    sender_fees: HashMap<UInt160, BigInt>,
    oracle_responses: HashSet<u64>,
}

impl ProposalVerificationContext {
    fn add_transaction(&mut self, tx: Arc<Transaction>) {
        let hash = tx.hash();
        if let Some(sender) = tx.signers().first().map(|signer| signer.account) {
            let fee = BigInt::from(tx.system_fee()) + BigInt::from(tx.network_fee());
            self.sender_fees
                .entry(sender)
                .and_modify(|total| *total += &fee)
                .or_insert(fee);
        }
        if let Some(id) = oracle_response_id(&tx) {
            self.oracle_responses.insert(id);
        }
        self.transactions.insert(hash, tx);
    }

    fn sender_fee(&self, tx: &Transaction) -> BigInt {
        tx.signers()
            .first()
            .and_then(|signer| self.sender_fees.get(&signer.account))
            .cloned()
            .unwrap_or_default()
    }

    fn has_oracle_response(&self, tx: &Transaction) -> bool {
        oracle_response_id(tx).is_some_and(|id| self.oracle_responses.contains(&id))
    }
}

#[derive(Debug, Default)]
struct ProposalTransactionAvailability {
    available: Vec<UInt256>,
    rejection_reason: Option<ChangeViewReason>,
}

/// The BFT threshold `M = N - (N-1)/3` used by C# dBFT.
fn dbft_bft_threshold(n: usize) -> usize {
    if n == 0 { 0 } else { n - (n - 1) / 3 }
}

fn dbft_multisig_verification_script(validators: &[ValidatorInfo]) -> Vec<u8> {
    if validators.is_empty() {
        return Vec::new();
    }

    let keys: Vec<ECPoint> = validators
        .iter()
        .map(|validator| validator.public_key.clone())
        .collect();
    RedeemScript::multi_sig_redeem_script_from_points(dbft_bft_threshold(keys.len()), &keys)
        .expect("valid dBFT validator set")
}

/// Mirrors C# `ConsensusContext.GetExpectedBlockSizeWithoutTransactions`.
fn expected_dbft_block_size_without_transactions(
    expected_transactions: usize,
    validators: &[ValidatorInfo],
) -> usize {
    let witness =
        Witness::new_with_scripts(Vec::new(), dbft_multisig_verification_script(validators));
    4 + 32
        + 32
        + 8
        + 8
        + 4
        + 1
        + 20
        + 1
        + witness.size()
        + SerializeHelper::get_var_size_usize(expected_transactions)
}

fn proposed_block_policy_rejection(
    hashes: &[UInt256],
    cache: &HashMap<UInt256, Arc<Transaction>>,
    validators: &[ValidatorInfo],
    settings: &ProtocolSettings,
) -> Option<ChangeViewReason> {
    let mut block_size = expected_dbft_block_size_without_transactions(hashes.len(), validators);
    let mut system_fee = 0i128;

    for hash in hashes {
        let tx = cache.get(hash)?;
        block_size = block_size.saturating_add(<Transaction as Serializable>::size(tx.as_ref()));
        system_fee += i128::from(tx.system_fee());
    }

    if block_size > settings.max_block_size as usize {
        warn!(
            target: "neo",
            block_size,
            max_block_size = settings.max_block_size,
            "rejected PrepareRequest: expected block size exceeds dBFT policy"
        );
        return Some(ChangeViewReason::BlockRejectedByPolicy);
    }

    if system_fee > i128::from(DBFT_MAX_BLOCK_SYSTEM_FEE) {
        warn!(
            target: "neo",
            system_fee,
            max_block_system_fee = DBFT_MAX_BLOCK_SYSTEM_FEE,
            "rejected PrepareRequest: expected block system fee exceeds dBFT policy"
        );
        return Some(ChangeViewReason::BlockRejectedByPolicy);
    }

    None
}

fn select_primary_proposal_transactions(
    candidates: Vec<PoolItem>,
    max_count: usize,
    cache: &mut HashMap<UInt256, Arc<Transaction>>,
    validators: &[ValidatorInfo],
    settings: &ProtocolSettings,
) -> Vec<UInt256> {
    let candidates: Vec<PoolItem> = candidates.into_iter().take(max_count).collect();
    let mut block_size =
        expected_dbft_block_size_without_transactions(candidates.len(), validators);
    let mut system_fee = 0i128;
    let mut hashes = Vec::with_capacity(candidates.len());

    for item in candidates {
        let next_block_size = block_size.saturating_add(<Transaction as Serializable>::size(
            item.transaction.as_ref(),
        ));
        if next_block_size > settings.max_block_size as usize {
            break;
        }

        let next_system_fee = system_fee + i128::from(item.transaction.system_fee());
        if next_system_fee > i128::from(DBFT_MAX_BLOCK_SYSTEM_FEE) {
            break;
        }

        block_size = next_block_size;
        system_fee = next_system_fee;
        let hash = item.hash();
        cache.insert(hash, Arc::clone(&item.transaction));
        hashes.push(hash);
    }

    hashes
}

fn conflict_hashes(tx: &Transaction) -> impl Iterator<Item = UInt256> + '_ {
    tx.attributes()
        .iter()
        .filter_map(|attribute| match attribute {
            TransactionAttribute::Conflicts(conflict) => Some(conflict.hash),
            _ => None,
        })
}

fn oracle_response_id(tx: &Transaction) -> Option<u64> {
    tx.attributes()
        .iter()
        .find_map(|attribute| match attribute {
            TransactionAttribute::OracleResponse(response) => Some(response.id),
            _ => None,
        })
}

fn verify_unverified_proposal_transaction(
    tx: &Transaction,
    proposal_hashes: &HashSet<UInt256>,
    context: &ProposalVerificationContext,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
) -> VerifyResult {
    if conflict_hashes(tx).any(|hash| proposal_hashes.contains(&hash)) {
        return VerifyResult::HasConflicts;
    }
    if context
        .transactions
        .values()
        .any(|accepted| conflict_hashes(accepted).any(|hash| hash == tx.hash()))
    {
        return VerifyResult::HasConflicts;
    }

    let sender_fee = context.sender_fee(tx);
    verify_transaction(
        tx,
        snapshot,
        settings,
        &sender_fee,
        context.has_oracle_response(tx),
    )
}

fn proposal_rejection_reason(result: VerifyResult) -> ChangeViewReason {
    if result == VerifyResult::PolicyFail {
        ChangeViewReason::TxRejectedByPolicy
    } else {
        ChangeViewReason::TxInvalid
    }
}

/// Caches the full transactions named by a primary proposal and returns the
/// subset currently available locally.
///
/// C# DBFT `OnPrepareRequestReceived` first accepts already-verified mempool
/// transactions, then re-verifies unverified matches with the proposal-local
/// `TransactionVerificationContext` (`AddTransaction(tx, true)`). That context
/// catches proposal-internal conflicts, duplicated oracle responses, and sender
/// fee exhaustion across transactions before the backup reports availability.
fn cache_available_proposal_transactions(
    hashes: &[UInt256],
    cache: &mut HashMap<UInt256, Arc<Transaction>>,
    mempool: &MemoryPool,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    validators: &[ValidatorInfo],
) -> ProposalTransactionAvailability {
    let proposal_hashes: HashSet<UInt256> = hashes.iter().copied().collect();
    let mut context = ProposalVerificationContext::default();
    let mut unverified = Vec::new();
    let mut result = ProposalTransactionAvailability {
        available: Vec::with_capacity(hashes.len()),
        rejection_reason: None,
    };

    for hash in hashes {
        if let Some(item) = mempool.get_verified(hash) {
            cache.insert(*hash, Arc::clone(&item.transaction));
            result.available.push(*hash);
            context.add_transaction(item.transaction);
        } else if let Some(item) = mempool.get(hash) {
            unverified.push((*hash, item.transaction));
        }
    }

    for (hash, tx) in unverified {
        let verify_result = verify_unverified_proposal_transaction(
            &tx,
            &proposal_hashes,
            &context,
            snapshot,
            settings,
        );
        if verify_result != VerifyResult::Succeed {
            warn!(
                target: "neo",
                %hash,
                ?verify_result,
                "unverified PrepareRequest transaction failed proposal-context verification"
            );
            result.rejection_reason = Some(proposal_rejection_reason(verify_result));
            return result;
        }
        cache.insert(hash, Arc::clone(&tx));
        result.available.push(hash);
        context.add_transaction(tx);
    }

    if result.available.len() == hashes.len() {
        result.rejection_reason =
            proposed_block_policy_rejection(hashes, cache, validators, settings);
        if result.rejection_reason.is_some() {
            result.available.clear();
        }
    }

    result
}

/// C# DBFT `OnPrepareRequestReceived` rejects proposals that name a transaction
/// already persisted in Ledger, and rejects available local transactions whose
/// hash has a traceable on-chain conflict record.
fn prepare_request_passes_ledger_guards(
    payload: &ConsensusPayload,
    snapshot: &DataCache,
    mempool: &MemoryPool,
    settings: &ProtocolSettings,
) -> bool {
    if payload.message_type != ConsensusMessageType::PrepareRequest {
        return true;
    }

    let request = match PrepareRequestMessage::deserialize_body(
        &payload.data,
        payload.block_index,
        payload.view_number,
        payload.validator_index,
    ) {
        Ok(request) => request,
        Err(_) => return true,
    };

    let ledger = LedgerContract::new();
    for hash in &request.transaction_hashes {
        match ledger.contains_transaction(snapshot, hash) {
            Ok(true) => {
                warn!(target: "neo", %hash, "rejected PrepareRequest: transaction already exists on-chain");
                return false;
            }
            Ok(false) => {}
            Err(error) => {
                warn!(target: "neo", %hash, %error, "failed to check PrepareRequest transaction existence");
                return false;
            }
        }
    }

    let max_traceable_blocks = match PolicyContract::new()
        .get_max_traceable_blocks_snapshot(snapshot, settings)
    {
        Ok(value) => value,
        Err(error) => {
            warn!(target: "neo", %error, "failed to read MaxTraceableBlocks for PrepareRequest guard");
            return false;
        }
    };

    for hash in &request.transaction_hashes {
        let Some(item) = mempool.get(hash) else {
            continue;
        };
        let signers: Vec<UInt160> = item
            .transaction
            .signers()
            .iter()
            .map(|signer| signer.account)
            .collect();
        match ledger.contains_conflict_hash(snapshot, hash, &signers, max_traceable_blocks) {
            Ok(true) => {
                warn!(target: "neo", %hash, "rejected PrepareRequest: transaction has on-chain conflict");
                return false;
            }
            Ok(false) => {}
            Err(error) => {
                warn!(target: "neo", %hash, %error, "failed to check PrepareRequest transaction conflict");
                return false;
            }
        }
    }

    true
}

/// Reads the current ledger tip from `snapshot` →
/// `(next_block_index, prev_hash, prev_timestamp)`.
fn ledger_tip(snapshot: &DataCache) -> (u32, UInt256, u64) {
    let ledger = LedgerContract::new();
    let height = ledger.current_index(snapshot).unwrap_or(0);
    let prev_hash = ledger.current_hash(snapshot).unwrap_or_default();
    let prev_timestamp = ledger
        .get_trimmed_block(snapshot, &prev_hash)
        .ok()
        .flatten()
        .map(|block| block.header.timestamp())
        .unwrap_or(0);
    (height + 1, prev_hash, prev_timestamp)
}

fn round_validator_context(
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    block_index: u32,
) -> anyhow::Result<(Vec<ValidatorInfo>, UInt160)> {
    let validators_count = usize::try_from(settings.validators_count).unwrap_or(0);
    let validators = validator_infos_from_keys(
        NeoToken::new().next_block_validators(snapshot, validators_count)?,
    );
    let next_consensus =
        NeoToken::new().next_consensus_address_for_block(snapshot, settings, block_index)?;
    Ok((validators, next_consensus))
}

/// The single-task consensus driver: owns the `ConsensusService` (so no lock is
/// needed) and routes its events to the network/mempool/ledger.
struct ConsensusDriver {
    service: ConsensusService,
    event_rx: mpsc::Receiver<ConsensusEvent>,
    inbound_rx: mpsc::Receiver<ConsensusPayload>,
    blockchain: BlockchainHandle,
    mempool: Arc<MemoryPool>,
    network: NetworkHandle,
    settings: Arc<ProtocolSettings>,
    validators: Arc<RwLock<Vec<ValidatorInfo>>>,
    public_key: ECPoint,
    /// The `prev_hash` of the round currently being driven (carried into
    /// `assemble_block` — the stored snapshot does not advance, so the tip is
    /// tracked from `start`/`Imported` rather than re-read).
    current_prev_hash: UInt256,
    /// Full transactions cached at proposal time, for commit-time assembly.
    proposal_txs: HashMap<UInt256, Arc<Transaction>>,
}

impl ConsensusDriver {
    fn configure_round(
        &mut self,
        snapshot: &DataCache,
        block_index: u32,
    ) -> anyhow::Result<UInt160> {
        let (validators, next_consensus) =
            round_validator_context(snapshot, &self.settings, block_index)?;
        let my_index = resolve_public_key_index(&self.public_key, &validators);
        self.service.update_validators(validators.clone(), my_index);
        *self.validators.write() = validators;
        Ok(next_consensus)
    }

    async fn run(mut self, startup_snapshot: Arc<DataCache>) {
        // C# `ConsensusContext.Reset`: first round is height+1 over the tip.
        let (block_index, prev_hash, prev_timestamp) = ledger_tip(&startup_snapshot);
        let next_consensus = match self.configure_round(&startup_snapshot, block_index) {
            Ok(next_consensus) => next_consensus,
            Err(err) => {
                warn!(target: "neo", %err, "consensus round context unavailable");
                return;
            }
        };
        self.current_prev_hash = prev_hash;
        match self.service.start_with_block_context(
            block_index,
            now_millis(),
            prev_hash,
            prev_timestamp,
            next_consensus,
            BLOCK_VERSION,
        ) {
            Ok(()) => info!(target: "neo", block_index, "consensus started"),
            Err(err) => {
                info!(target: "neo", %err, block_index, "consensus not started; driver idle");
            }
        }

        let mut persist_rx = self.blockchain.subscribe();
        let mut ticker = tokio::time::interval(Duration::from_millis(1_000));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                // Outbound work from the state machine.
                maybe_event = self.event_rx.recv() => {
                    let Some(event) = maybe_event else { break };
                    self.on_consensus_event(event, &startup_snapshot).await;
                }
                // Inbound consensus payloads from peers.
                maybe_msg = self.inbound_rx.recv() => {
                    let Some(payload) = maybe_msg else { break };
                    if !prepare_request_passes_ledger_guards(
                        &payload,
                        &startup_snapshot,
                        &self.mempool,
                        &self.settings,
                    ) {
                        continue;
                    }
                    if let Err(err) = self.service.process_message(payload) {
                        warn!(target: "neo", %err, "consensus rejected inbound payload");
                    }
                }
                // A block persisted (locally committed or peer-synced) → next round.
                ev = persist_rx.recv() => {
                    match ev {
                        Ok(RuntimeEvent::Imported { hash, height, timestamp }) => {
                            let block_index = height + 1;
                            let next_consensus = match self.configure_round(&startup_snapshot, block_index) {
                                Ok(next_consensus) => next_consensus,
                                Err(err) => {
                                    warn!(target: "neo", %err, block_index, "consensus round context unavailable");
                                    continue;
                                }
                            };
                            self.current_prev_hash = hash;
                            self.proposal_txs.clear();
                            match self.service.start_with_block_context(
                                block_index,
                                now_millis(),
                                hash,
                                timestamp,
                                next_consensus,
                                BLOCK_VERSION,
                            ) {
                                Ok(()) => info!(target: "neo", block_index, "consensus restarted"),
                                Err(err) => info!(target: "neo", %err, "consensus next round not started"),
                            }
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                // View-timeout tick (the real deadline lives inside the context).
                _ = ticker.tick() => {
                    if let Err(err) = self.service.on_timer_tick(now_millis()) {
                        warn!(target: "neo", %err, "consensus timer tick failed");
                    }
                }
            }
        }
        info!(target: "neo", "consensus driver loop exited");
    }

    async fn on_consensus_event(&mut self, event: ConsensusEvent, snapshot: &DataCache) {
        match event {
            ConsensusEvent::BroadcastMessage(payload) => {
                let ext = {
                    let validators = self.validators.read();
                    consensus_to_extensible(&payload, &validators)
                };
                if let Some(ext) = ext {
                    let _ = self.network.broadcast_extensible(ext).await;
                }
            }
            ConsensusEvent::RequestTransactions { max_count, .. } => {
                let hashes = {
                    let validators = self.validators.read();
                    select_primary_proposal_transactions(
                        self.mempool.verified_snapshot(),
                        max_count,
                        &mut self.proposal_txs,
                        &validators,
                        &self.settings,
                    )
                };
                if let Err(err) = self.service.on_transactions_received(hashes) {
                    warn!(target: "neo", %err, "consensus on_transactions_received failed");
                }
            }
            ConsensusEvent::RequestProposalTransactions {
                transaction_hashes, ..
            } => {
                let availability = {
                    let validators = self.validators.read();
                    cache_available_proposal_transactions(
                        &transaction_hashes,
                        &mut self.proposal_txs,
                        &self.mempool,
                        snapshot,
                        &self.settings,
                        &validators,
                    )
                };
                if let Err(err) = self
                    .service
                    .on_transactions_received(availability.available)
                {
                    warn!(target: "neo", %err, "consensus on_transactions_received failed");
                }
                if let Some(reason) = availability.rejection_reason {
                    if let Err(err) = self.service.request_change_view(reason, now_millis()) {
                        warn!(target: "neo", %err, ?reason, "consensus request_change_view failed");
                    }
                }
            }
            ConsensusEvent::BlockCommitted {
                block_index,
                block_data,
                ..
            } => {
                let txs = match resolve_transactions(
                    &block_data.transaction_hashes,
                    &self.proposal_txs,
                    &self.mempool,
                ) {
                    Some(txs) => txs,
                    None => {
                        error!(target: "neo", block_index, "missing transaction for committed block; cannot assemble");
                        return;
                    }
                };
                match block_data.assemble_block(BLOCK_VERSION, self.current_prev_hash, txs) {
                    Ok(block) => {
                        self.current_prev_hash = block.header.hash();
                        let block = Arc::new(block);
                        // Persist through the C# Blockchain.Persist pipeline; the
                        // validators already signed, so it is pre-verified.
                        let _ = self
                            .blockchain
                            .tell(BlockchainCommand::InventoryBlock {
                                block: Arc::clone(&block),
                                relay: true,
                                pre_verified: true,
                            })
                            .await;
                        // The InventoryBlock handler does not relay, so broadcast
                        // the new block to peers explicitly.
                        let _ = self.network.broadcast_block((*block).clone()).await;
                        info!(target: "neo", block_index, "consensus produced + submitted block");
                        // The next round restarts off the RuntimeEvent::Imported.
                    }
                    Err(err) => {
                        error!(target: "neo", block_index, %err, "consensus block assembly failed")
                    }
                }
            }
            ConsensusEvent::ViewChanged {
                block_index,
                old_view,
                new_view,
            } => {
                info!(target: "neo", block_index, old_view, new_view, "consensus view changed");
            }
        }
    }
}

/// Spawns the consensus driver for a validator node, consuming the caller-owned
/// `inbound_rx` (its matching sender is wired into the network forwarder before
/// this is called — the network, and thus this driver, is built after the
/// forwarder). Returns the driver task handle, or `None` when this node is not
/// a consensus validator (relay-only).
pub fn spawn_consensus_driver(
    setup: ConsensusSetup,
    blockchain: BlockchainHandle,
    mempool: Arc<MemoryPool>,
    network: NetworkHandle,
    settings: Arc<ProtocolSettings>,
    validators: Arc<RwLock<Vec<ValidatorInfo>>>,
    startup_snapshot: Arc<DataCache>,
    inbound_rx: mpsc::Receiver<ConsensusPayload>,
) -> Option<tokio::task::JoinHandle<()>> {
    // Generously sized: a commit emits BroadcastMessage(Commit) + BlockCommitted
    // back-to-back via the consensus crate's non-blocking try_send.
    let (event_tx, event_rx) = mpsc::channel::<ConsensusEvent>(1024);

    let mut service = ConsensusService::new(
        setup.network,
        setup.validators.clone(),
        setup.my_index,
        setup.private_key.to_vec(),
        event_tx,
    );
    // When an HSM-backed signer is configured, route consensus signing through
    // it (the software private_key above is zeroed and unused in that case).
    service.set_signer(setup.signer.clone());
    service.set_expected_block_time(setup.ms_per_block);
    service.set_max_transactions_per_block(settings.max_transactions_per_block);

    let driver = ConsensusDriver {
        service,
        event_rx,
        inbound_rx,
        blockchain,
        mempool,
        network,
        settings,
        validators,
        public_key: setup.public_key,
        current_prev_hash: UInt256::default(),
        proposal_txs: HashMap::new(),
    };

    Some(tokio::spawn(driver.run(startup_snapshot)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_consensus::{
        ChangeViewMessage, ChangeViewReason, ConsensusContext, ConsensusMessageType,
        messages::PrepareRequestMessage,
    };
    use neo_crypto::signature::Secp256r1Crypto;
    use neo_payloads::{Signer, Transaction, TransactionAttribute, Witness};
    use neo_primitives::{UInt160, VerifyResult, WitnessScope};
    use neo_serialization::BinarySerializer;
    use neo_storage::{StorageItem, StorageKey};
    use neo_vm::StackItem;
    use neo_vm_rs::ExecutionEngineLimits;
    use neo_vm_rs::OpCode;

    /// The dBFT extensible codec round-trips a consensus payload: encode a
    /// signed `ConsensusPayload` to an `ExtensiblePayload`, then decode it back
    /// to the same fields (the inbound path authenticates the sender).
    #[test]
    fn extensible_codec_round_trips() {
        let settings = ProtocolSettings::default();
        let validators = build_consensus_validators(&settings);
        assert!(!validators.is_empty(), "default settings carry a committee");

        let validator_index = 0u8;
        let signature = vec![0xABu8; 64];
        let mut original = ConsensusPayload::new(
            settings.network,
            7, // block_index
            validator_index,
            0, // view_number
            neo_consensus::ConsensusMessageType::Commit,
            vec![0x01, 0x02, 0x03], // body
        );
        original.witness = signature.clone();

        let ext = consensus_to_extensible(&original, &validators).expect("encode");
        assert_eq!(ext.category, DBFT_CATEGORY);
        assert_eq!(ext.valid_block_end, 7);
        assert_eq!(ext.sender, validators[validator_index as usize].script_hash);

        let decoded = extensible_to_consensus(&ext, settings.network, &validators).expect("decode");
        assert_eq!(decoded.block_index, 7);
        assert_eq!(decoded.validator_index, validator_index);
        assert_eq!(
            decoded.message_type,
            neo_consensus::ConsensusMessageType::Commit
        );
        assert_eq!(decoded.data, vec![0x01, 0x02, 0x03]);
        assert_eq!(decoded.witness, signature);
    }

    /// A non-dBFT extensible is ignored by the consensus decoder.
    #[test]
    fn extensible_codec_rejects_non_dbft() {
        let settings = ProtocolSettings::default();
        let validators = build_consensus_validators(&settings);
        let mut ext = ExtensiblePayload::new();
        ext.category = "StateService".to_string();
        ext.valid_block_end = 1;
        assert!(extensible_to_consensus(&ext, settings.network, &validators).is_none());
    }

    /// The validator set is sorted ascending by public key (consensus-critical:
    /// the index order drives primary selection + NextConsensus).
    #[test]
    fn validators_are_sorted_by_pubkey() {
        let settings = ProtocolSettings::default();
        let validators = build_consensus_validators(&settings);
        for pair in validators.windows(2) {
            assert!(
                pair[0].public_key <= pair[1].public_key,
                "validators must be sorted"
            );
        }
        for (i, v) in validators.iter().enumerate() {
            assert_eq!(v.index as usize, i);
        }
    }

    #[test]
    fn proposal_resolution_caches_unverified_transactions_like_csharp_prepare_request() {
        neo_native_contracts::install();
        let settings = ProtocolSettings::default();
        let snapshot = DataCache::new(false);
        let pool = MemoryPool::new(&settings);
        seed_current_block(&snapshot, 0);
        set_zero_policy_fee(&snapshot, 10);
        set_zero_policy_fee(&snapshot, 18);

        let tx = signed_zero_fee_tx(&settings, 0x32);
        let hash = tx.hash();
        assert_eq!(pool.try_add(tx.clone(), &snapshot), VerifyResult::Succeed);
        pool.update_pool_for_block_persisted(&[]);
        assert!(pool.get_verified(&hash).is_none());
        assert!(pool.get(&hash).is_some());

        let mut cache: HashMap<UInt256, Arc<Transaction>> = HashMap::new();
        let (validators, _) = consensus_test_validators(4);
        let available = cache_available_proposal_transactions(
            &[hash],
            &mut cache,
            &pool,
            &snapshot,
            &settings,
            &validators,
        );
        assert_eq!(available.available, vec![hash]);
        assert_eq!(available.rejection_reason, None);
        assert_eq!(cache.get(&hash).map(|tx| tx.hash()), Some(hash));
    }

    #[test]
    fn proposal_resolution_reverifies_unverified_transactions_against_context() {
        neo_native_contracts::install();
        let settings = ProtocolSettings::default();
        let snapshot = DataCache::new(false);
        let pool = MemoryPool::new(&settings);
        seed_current_block(&snapshot, 0);
        set_zero_policy_fee(&snapshot, 10);
        set_zero_policy_fee(&snapshot, 18);

        let private = [0x51u8; 32];
        let public = Secp256r1Crypto::derive_public_key(&private).expect("pubkey");
        let verification =
            neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public);
        let account = UInt160::from_script(&verification);
        seed_gas_balance(&snapshot, &account, 2_000);

        let first = signed_tx_with_fees(
            &settings,
            &private,
            &public,
            account,
            0x5100_0001,
            0,
            1_000,
            Vec::new(),
        );
        let second = signed_tx_with_fees(
            &settings,
            &private,
            &public,
            account,
            0x5100_0002,
            0,
            1_000,
            Vec::new(),
        );
        let first_hash = first.hash();
        let second_hash = second.hash();
        assert_eq!(
            pool.try_add(first.clone(), &snapshot),
            VerifyResult::Succeed
        );
        assert_eq!(
            pool.try_add(second.clone(), &snapshot),
            VerifyResult::Succeed
        );
        pool.update_pool_for_block_persisted(&[]);
        assert!(pool.get_verified(&first_hash).is_none());
        assert!(pool.get_verified(&second_hash).is_none());

        seed_gas_balance(&snapshot, &account, 1_500);
        let mut cache: HashMap<UInt256, Arc<Transaction>> = HashMap::new();
        let (validators, _) = consensus_test_validators(4);
        let available = cache_available_proposal_transactions(
            &[first_hash, second_hash],
            &mut cache,
            &pool,
            &snapshot,
            &settings,
            &validators,
        );

        assert_eq!(
            available.available,
            vec![first_hash],
            "C# AddTransaction(tx, true) re-verifies unverified proposal txs against context sender fees"
        );
        assert_eq!(
            available.rejection_reason,
            Some(ChangeViewReason::TxInvalid)
        );
        assert!(cache.contains_key(&first_hash));
        assert!(!cache.contains_key(&second_hash));
    }

    #[test]
    fn proposal_resolution_rejects_unverified_conflicts_against_proposal_hashes() {
        neo_native_contracts::install();
        let settings = ProtocolSettings::default();
        let snapshot = DataCache::new(false);
        let pool = MemoryPool::new(&settings);
        seed_current_block(&snapshot, 0);
        set_zero_policy_fee(&snapshot, 10);
        set_zero_policy_fee(&snapshot, 18);

        let target = signed_zero_fee_tx(&settings, 0x52);
        let target_hash = target.hash();
        let (private, public, account) = signing_account(0x53);
        let conflict = signed_tx_with_fees(
            &settings,
            &private,
            &public,
            account,
            0x5300_0001,
            0,
            0,
            vec![TransactionAttribute::Conflicts(
                neo_payloads::Conflicts::new(target_hash),
            )],
        );
        let conflict_hash = conflict.hash();

        assert_eq!(pool.try_add(conflict, &snapshot), VerifyResult::Succeed);
        pool.update_pool_for_block_persisted(&[]);
        assert!(pool.get_verified(&conflict_hash).is_none());
        assert_eq!(pool.try_add(target, &snapshot), VerifyResult::Succeed);
        assert!(pool.get_verified(&target_hash).is_some());

        let mut cache: HashMap<UInt256, Arc<Transaction>> = HashMap::new();
        let (validators, _) = consensus_test_validators(4);
        let available = cache_available_proposal_transactions(
            &[target_hash, conflict_hash],
            &mut cache,
            &pool,
            &snapshot,
            &settings,
            &validators,
        );

        assert_eq!(available.available, vec![target_hash]);
        assert_eq!(
            available.rejection_reason,
            Some(ChangeViewReason::TxInvalid)
        );
        assert!(cache.contains_key(&target_hash));
        assert!(!cache.contains_key(&conflict_hash));
    }

    #[test]
    fn proposal_resolution_rejects_unverified_when_context_conflicts_with_it() {
        neo_native_contracts::install();
        let settings = ProtocolSettings::default();
        let snapshot = DataCache::new(false);
        let pool = MemoryPool::new(&settings);
        seed_current_block(&snapshot, 0);
        set_zero_policy_fee(&snapshot, 10);
        set_zero_policy_fee(&snapshot, 18);

        let target = signed_zero_fee_tx(&settings, 0x54);
        let target_hash = target.hash();
        assert_eq!(pool.try_add(target, &snapshot), VerifyResult::Succeed);
        pool.update_pool_for_block_persisted(&[]);
        assert!(pool.get_verified(&target_hash).is_none());

        let (private, public, account) = signing_account(0x55);
        let conflict = signed_tx_with_fees(
            &settings,
            &private,
            &public,
            account,
            0x5500_0001,
            0,
            0,
            vec![TransactionAttribute::Conflicts(
                neo_payloads::Conflicts::new(target_hash),
            )],
        );
        let conflict_hash = conflict.hash();
        assert_eq!(pool.try_add(conflict, &snapshot), VerifyResult::Succeed);
        assert!(pool.get_verified(&conflict_hash).is_some());

        let mut cache: HashMap<UInt256, Arc<Transaction>> = HashMap::new();
        let (validators, _) = consensus_test_validators(4);
        let available = cache_available_proposal_transactions(
            &[conflict_hash, target_hash],
            &mut cache,
            &pool,
            &snapshot,
            &settings,
            &validators,
        );

        assert_eq!(available.available, vec![conflict_hash]);
        assert_eq!(
            available.rejection_reason,
            Some(ChangeViewReason::TxInvalid)
        );
        assert!(cache.contains_key(&conflict_hash));
        assert!(!cache.contains_key(&target_hash));
    }

    #[test]
    fn primary_proposal_selection_stops_before_dbft_max_block_system_fee() {
        let settings = ProtocolSettings::default();
        let (validators, _) = consensus_test_validators(4);
        let (private, public, account) = signing_account(0x61);
        let first = signed_tx_with_fees(
            &settings,
            &private,
            &public,
            account,
            0x6100_0001,
            DBFT_MAX_BLOCK_SYSTEM_FEE,
            0,
            Vec::new(),
        );
        let second = signed_tx_with_fees(
            &settings,
            &private,
            &public,
            account,
            0x6100_0002,
            1,
            0,
            Vec::new(),
        );
        let first_hash = first.hash();
        let second_hash = second.hash();
        let mut cache: HashMap<UInt256, Arc<Transaction>> = HashMap::new();

        let hashes = select_primary_proposal_transactions(
            vec![PoolItem::new(first), PoolItem::new(second)],
            2,
            &mut cache,
            &validators,
            &settings,
        );

        assert_eq!(hashes, vec![first_hash]);
        assert!(cache.contains_key(&first_hash));
        assert!(
            !cache.contains_key(&second_hash),
            "C# EnsureMaxBlockLimitation breaks before adding the tx that would exceed MaxBlockSystemFee"
        );
    }

    #[test]
    fn proposal_resolution_rejects_full_block_over_dbft_max_block_size() {
        neo_native_contracts::install();
        let mut settings = ProtocolSettings::default();
        let snapshot = DataCache::new(false);
        let pool = MemoryPool::new(&settings);
        seed_current_block(&snapshot, 0);
        set_zero_policy_fee(&snapshot, 10);
        set_zero_policy_fee(&snapshot, 18);
        let (validators, _) = consensus_test_validators(4);
        let tx = signed_zero_fee_tx(&settings, 0x62);
        let hash = tx.hash();
        let oversized_limit = expected_dbft_block_size_without_transactions(1, &validators)
            + <Transaction as Serializable>::size(&tx)
            - 1;
        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);
        settings.max_block_size = oversized_limit as u32;

        let mut cache: HashMap<UInt256, Arc<Transaction>> = HashMap::new();
        let available = cache_available_proposal_transactions(
            &[hash],
            &mut cache,
            &pool,
            &snapshot,
            &settings,
            &validators,
        );

        assert!(available.available.is_empty());
        assert_eq!(
            available.rejection_reason,
            Some(ChangeViewReason::BlockRejectedByPolicy)
        );
    }

    #[test]
    fn proposal_rejection_reason_matches_csharp_add_transaction_mapping() {
        assert_eq!(
            proposal_rejection_reason(VerifyResult::PolicyFail),
            ChangeViewReason::TxRejectedByPolicy
        );
        assert_eq!(
            proposal_rejection_reason(VerifyResult::HasConflicts),
            ChangeViewReason::TxInvalid
        );
        assert_eq!(
            proposal_rejection_reason(VerifyResult::InsufficientFunds),
            ChangeViewReason::TxInvalid
        );
    }

    #[tokio::test]
    async fn proposal_resolution_requests_change_view_for_invalid_unverified_transaction() {
        neo_native_contracts::install();
        let settings = Arc::new(ProtocolSettings::default());
        let snapshot = DataCache::new(false);
        let pool = Arc::new(MemoryPool::new(&settings));
        seed_current_block(&snapshot, 0);
        set_zero_policy_fee(&snapshot, 10);
        set_zero_policy_fee(&snapshot, 18);

        let private = [0x56u8; 32];
        let public = Secp256r1Crypto::derive_public_key(&private).expect("pubkey");
        let verification =
            neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public);
        let account = UInt160::from_script(&verification);
        seed_gas_balance(&snapshot, &account, 2_000);

        let first = signed_tx_with_fees(
            &settings,
            &private,
            &public,
            account,
            0x5600_0001,
            0,
            1_000,
            Vec::new(),
        );
        let second = signed_tx_with_fees(
            &settings,
            &private,
            &public,
            account,
            0x5600_0002,
            0,
            1_000,
            Vec::new(),
        );
        let first_hash = first.hash();
        let second_hash = second.hash();
        assert_eq!(pool.try_add(first, &snapshot), VerifyResult::Succeed);
        assert_eq!(pool.try_add(second, &snapshot), VerifyResult::Succeed);
        pool.update_pool_for_block_persisted(&[]);
        seed_gas_balance(&snapshot, &account, 1_500);

        let (validators, consensus_keys) = consensus_test_validators(4);
        let (event_tx, event_rx) = mpsc::channel(16);
        let mut context = ConsensusContext::new(0, validators.clone(), Some(1), Some(1_000));
        context.prepare_request_received = true;
        context.proposed_tx_hashes = vec![first_hash, second_hash];
        let mut service = ConsensusService::with_context(
            settings.network,
            context,
            consensus_keys[1].to_vec(),
            event_tx,
        );
        service
            .resume_with_next_consensus(10_000, UInt256::zero(), UInt160::zero(), 0)
            .expect("resume backup context");

        let (blockchain, _blockchain_rx) = BlockchainHandle::with_capacity();
        let (network, _network_rx, _network_events) = NetworkHandle::channel(16, 16);
        let (_inbound_tx, inbound_rx) = mpsc::channel(16);
        let mut driver = ConsensusDriver {
            service,
            event_rx,
            inbound_rx,
            blockchain,
            mempool: pool,
            network,
            settings,
            validators: Arc::new(RwLock::new(validators.clone())),
            public_key: validators[1].public_key.clone(),
            current_prev_hash: UInt256::default(),
            proposal_txs: HashMap::new(),
        };

        driver
            .on_consensus_event(
                ConsensusEvent::RequestProposalTransactions {
                    block_index: 0,
                    transaction_hashes: vec![first_hash, second_hash],
                },
                &snapshot,
            )
            .await;

        let mut reason = None;
        while let Ok(event) = driver.event_rx.try_recv() {
            if let ConsensusEvent::BroadcastMessage(payload) = event {
                if payload.message_type == ConsensusMessageType::ChangeView {
                    let msg = ChangeViewMessage::deserialize(
                        &payload.data,
                        payload.block_index,
                        payload.view_number,
                        payload.validator_index,
                    )
                    .expect("change view deserialize");
                    reason = Some(msg.reason);
                    break;
                }
            }
        }

        assert_eq!(
            reason,
            Some(ChangeViewReason::TxInvalid),
            "C# AddTransaction(tx, true) requests TxInvalid when proposal-local re-verification fails"
        );
    }

    #[tokio::test]
    async fn proposal_resolution_requests_block_rejected_without_prepare_response_for_over_fee_block()
     {
        neo_native_contracts::install();
        let settings = Arc::new(ProtocolSettings::default());
        let snapshot = DataCache::new(false);
        let pool = Arc::new(MemoryPool::new(&settings));
        seed_current_block(&snapshot, 0);
        set_zero_policy_fee(&snapshot, 10);
        set_zero_policy_fee(&snapshot, 18);

        let private = [0x63u8; 32];
        let public = Secp256r1Crypto::derive_public_key(&private).expect("pubkey");
        let verification =
            neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public);
        let account = UInt160::from_script(&verification);
        seed_gas_balance(&snapshot, &account, DBFT_MAX_BLOCK_SYSTEM_FEE + 100);

        let first = signed_tx_with_fees(
            &settings,
            &private,
            &public,
            account,
            0x6300_0001,
            DBFT_MAX_BLOCK_SYSTEM_FEE,
            0,
            Vec::new(),
        );
        let second = signed_tx_with_fees(
            &settings,
            &private,
            &public,
            account,
            0x6300_0002,
            1,
            0,
            Vec::new(),
        );
        let first_hash = first.hash();
        let second_hash = second.hash();
        assert_eq!(pool.try_add(first, &snapshot), VerifyResult::Succeed);
        assert_eq!(pool.try_add(second, &snapshot), VerifyResult::Succeed);

        let (validators, consensus_keys) = consensus_test_validators(4);
        let (event_tx, event_rx) = mpsc::channel(16);
        let mut context = ConsensusContext::new(0, validators.clone(), Some(1), Some(1_000));
        context.prepare_request_received = true;
        context.proposed_tx_hashes = vec![first_hash, second_hash];
        let mut service = ConsensusService::with_context(
            settings.network,
            context,
            consensus_keys[1].to_vec(),
            event_tx,
        );
        service
            .resume_with_next_consensus(10_000, UInt256::zero(), UInt160::zero(), 0)
            .expect("resume backup context");

        let (blockchain, _blockchain_rx) = BlockchainHandle::with_capacity();
        let (network, _network_rx, _network_events) = NetworkHandle::channel(16, 16);
        let (_inbound_tx, inbound_rx) = mpsc::channel(16);
        let mut driver = ConsensusDriver {
            service,
            event_rx,
            inbound_rx,
            blockchain,
            mempool: pool,
            network,
            settings,
            validators: Arc::new(RwLock::new(validators.clone())),
            public_key: validators[1].public_key.clone(),
            current_prev_hash: UInt256::default(),
            proposal_txs: HashMap::new(),
        };

        driver
            .on_consensus_event(
                ConsensusEvent::RequestProposalTransactions {
                    block_index: 0,
                    transaction_hashes: vec![first_hash, second_hash],
                },
                &snapshot,
            )
            .await;

        let mut reason = None;
        let mut sent_prepare_response = false;
        while let Ok(event) = driver.event_rx.try_recv() {
            if let ConsensusEvent::BroadcastMessage(payload) = event {
                match payload.message_type {
                    ConsensusMessageType::ChangeView => {
                        let msg = ChangeViewMessage::deserialize(
                            &payload.data,
                            payload.block_index,
                            payload.view_number,
                            payload.validator_index,
                        )
                        .expect("change view deserialize");
                        reason = Some(msg.reason);
                    }
                    ConsensusMessageType::PrepareResponse => {
                        sent_prepare_response = true;
                    }
                    _ => {}
                }
            }
        }

        assert_eq!(reason, Some(ChangeViewReason::BlockRejectedByPolicy));
        assert!(
            !sent_prepare_response,
            "C# CheckPrepareResponse requests BlockRejectedByPolicy before sending PrepareResponse"
        );
    }

    #[test]
    fn prepare_request_ledger_guard_rejects_already_persisted_transaction_hash() {
        neo_native_contracts::install();
        let settings = ProtocolSettings::default();
        let snapshot = DataCache::new(false);
        let pool = MemoryPool::new(&settings);
        let tx = signed_zero_fee_tx(&settings, 0x40);
        seed_persisted_transaction(&snapshot, 7, &tx);

        let payload = prepare_request_payload(vec![tx.hash()]);

        assert!(
            !prepare_request_passes_ledger_guards(&payload, &snapshot, &pool, &settings),
            "C# OnPrepareRequestReceived returns before accepting a proposed on-chain tx"
        );
    }

    #[test]
    fn prepare_request_ledger_guard_rejects_available_transaction_with_onchain_conflict() {
        neo_native_contracts::install();
        let settings = ProtocolSettings::default();
        let snapshot = DataCache::new(false);
        let pool = MemoryPool::new(&settings);
        seed_current_block(&snapshot, 0);
        set_zero_policy_fee(&snapshot, 10);
        set_zero_policy_fee(&snapshot, 18);

        let tx = signed_zero_fee_tx(&settings, 0x41);
        let hash = tx.hash();
        let signer = tx.signers().first().expect("signer").account;
        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);
        seed_current_block(&snapshot, 100);
        seed_traceable_conflict(&snapshot, &hash, &signer, 95);

        let payload = prepare_request_payload(vec![hash]);

        assert!(
            !prepare_request_passes_ledger_guards(&payload, &snapshot, &pool, &settings),
            "C# OnPrepareRequestReceived rejects proposed txs with traceable on-chain conflicts"
        );
    }

    #[test]
    fn prepare_request_ledger_guard_uses_dynamic_max_traceable_blocks() {
        neo_native_contracts::install();
        let mut settings = ProtocolSettings::default();
        settings
            .hardforks
            .insert(neo_config::Hardfork::HfEchidna, 0);
        let snapshot = DataCache::new(false);
        let pool = MemoryPool::new(&settings);
        seed_current_block(&snapshot, 0);
        set_zero_policy_fee(&snapshot, 10);
        set_zero_policy_fee(&snapshot, 18);

        let tx = signed_zero_fee_tx(&settings, 0x42);
        let hash = tx.hash();
        let signer = tx.signers().first().expect("signer").account;
        assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);
        seed_current_block(&snapshot, 100);
        set_policy_u32(&snapshot, 23, 3);
        seed_traceable_conflict(&snapshot, &hash, &signer, 95);

        let payload = prepare_request_payload(vec![hash]);

        assert!(
            prepare_request_passes_ledger_guards(&payload, &snapshot, &pool, &settings),
            "Policy MaxTraceableBlocks=3 makes a block-95 conflict untraceable at height 100"
        );
    }

    /// `build_consensus_setup` is a no-op when disabled and errors when enabled
    /// without a key.
    #[test]
    fn setup_gating() {
        let settings = ProtocolSettings::default();
        assert!(
            build_consensus_setup(&settings, false, None, None)
                .unwrap()
                .is_none()
        );
        assert!(build_consensus_setup(&settings, true, None, None).is_err());
        assert!(build_consensus_setup(&settings, true, Some("zz"), None).is_err());

        let non_validator_key = hex::encode([0x11u8; 32]);
        let setup = build_consensus_setup(&settings, true, Some(&non_validator_key), None)
            .unwrap()
            .expect("consensus configured");
        assert!(
            setup.my_index.is_none(),
            "key is not in the startup validator set"
        );
        assert!(!setup.validators.is_empty());
    }

    fn set_zero_policy_fee(snapshot: &DataCache, prefix: u8) {
        snapshot.add(
            StorageKey::new(neo_native_contracts::PolicyContract::ID, vec![prefix]),
            StorageItem::from_bytes(Vec::new()),
        );
    }

    fn set_policy_u32(snapshot: &DataCache, prefix: u8, value: u32) {
        snapshot.add(
            StorageKey::new(neo_native_contracts::PolicyContract::ID, vec![prefix]),
            StorageItem::from_bytes(u32_to_native_storage_bytes(value)),
        );
    }

    fn u32_to_native_storage_bytes(value: u32) -> Vec<u8> {
        if value == 0 {
            return Vec::new();
        }

        let mut bytes = value.to_le_bytes().to_vec();
        while bytes.len() > 1 {
            let last = *bytes.last().expect("non-empty");
            let next = bytes[bytes.len() - 2];
            if last != 0 || next & 0x80 != 0 {
                break;
            }
            bytes.pop();
        }
        if bytes.last().expect("non-empty") & 0x80 != 0 {
            bytes.push(0);
        }
        bytes
    }

    fn prepare_request_payload(transaction_hashes: Vec<UInt256>) -> ConsensusPayload {
        let message =
            PrepareRequestMessage::new(1, 0, 0, 0, UInt256::default(), 1, 42, transaction_hashes);
        ConsensusPayload::new(
            ProtocolSettings::default().network,
            1,
            0,
            0,
            ConsensusMessageType::PrepareRequest,
            message.serialize(),
        )
    }

    fn seed_current_block(snapshot: &DataCache, index: u32) {
        let ledger = LedgerContract::new();
        snapshot.update(
            StorageKey::new(LedgerContract::ID, vec![12]),
            StorageItem::from_bytes(
                ledger
                    .serialize_hash_index_state(&UInt256::from_bytes(&[0x11; 32]).unwrap(), index)
                    .unwrap(),
            ),
        );
    }

    fn seed_persisted_transaction(snapshot: &DataCache, block_index: u32, tx: &Transaction) {
        let mut key = Vec::with_capacity(33);
        key.push(11);
        key.extend_from_slice(&tx.hash().to_bytes());
        snapshot.add(
            StorageKey::new(LedgerContract::ID, key),
            StorageItem::from_bytes(
                LedgerContract::new()
                    .serialize_persisted_transaction_state(
                        block_index,
                        neo_vm_rs::VmState::HALT,
                        tx,
                    )
                    .unwrap(),
            ),
        );
    }

    fn seed_traceable_conflict(
        snapshot: &DataCache,
        hash: &UInt256,
        signer: &UInt160,
        block_index: u32,
    ) {
        let ledger = LedgerContract::new();
        let stub = ledger.serialize_conflict_stub(block_index).unwrap();

        let mut bare_key = Vec::with_capacity(33);
        bare_key.push(11);
        bare_key.extend_from_slice(&hash.to_bytes());
        snapshot.add(
            StorageKey::new(LedgerContract::ID, bare_key),
            StorageItem::from_bytes(stub.clone()),
        );

        let mut signer_key = Vec::with_capacity(53);
        signer_key.push(11);
        signer_key.extend_from_slice(&hash.to_bytes());
        signer_key.extend_from_slice(&signer.to_bytes());
        snapshot.add(
            StorageKey::new(LedgerContract::ID, signer_key),
            StorageItem::from_bytes(stub),
        );
    }

    fn seed_gas_balance(snapshot: &DataCache, account: &UInt160, datoshi: i64) {
        let item = StackItem::from_struct(vec![StackItem::from_int(datoshi)]);
        let bytes = BinarySerializer::serialize(&item, &ExecutionEngineLimits::default()).unwrap();
        let mut key = vec![20u8];
        key.extend_from_slice(&account.to_bytes());
        snapshot.update(
            StorageKey::new(neo_native_contracts::GasToken::ID, key),
            StorageItem::from_bytes(bytes),
        );
    }

    fn signed_zero_fee_tx(settings: &ProtocolSettings, seed: u8) -> Transaction {
        let private = [seed; 32];
        let public = Secp256r1Crypto::derive_public_key(&private).expect("pubkey");
        let verification =
            neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public);
        let account = UInt160::from_script(&verification);

        let mut tx = Transaction::new();
        tx.set_nonce(u32::from(seed));
        tx.set_valid_until_block(1);
        tx.set_script(vec![OpCode::PUSH1.byte()]);
        tx.set_signers(vec![Signer::new(account, WitnessScope::NONE)]);

        let hash = tx.try_hash().expect("tx hash");
        let mut sign_data = settings.network.to_le_bytes().to_vec();
        sign_data.extend_from_slice(&hash.to_bytes());
        let signature = Secp256r1Crypto::sign(&sign_data, &private).expect("sign");

        let mut invocation = vec![OpCode::PUSHDATA1.byte(), 64];
        invocation.extend_from_slice(&signature);
        tx.set_witnesses(vec![Witness::new_with_scripts(invocation, verification)]);
        tx
    }

    fn signing_account(seed: u8) -> ([u8; 32], Vec<u8>, UInt160) {
        let private = [seed; 32];
        let public = Secp256r1Crypto::derive_public_key(&private).expect("pubkey");
        let verification =
            neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public);
        let account = UInt160::from_script(&verification);
        (private, public, account)
    }

    fn consensus_test_validators(count: usize) -> (Vec<ValidatorInfo>, Vec<[u8; 32]>) {
        let mut validators = Vec::with_capacity(count);
        let mut private_keys = Vec::with_capacity(count);

        for index in 0..count {
            let private = [index as u8 + 1; 32];
            let public = Secp256r1Crypto::derive_public_key(&private).expect("pubkey");
            let public_key = ECPoint::from_bytes(&public).expect("ecpoint");
            let verification =
                neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
                    &public,
                );
            validators.push(ValidatorInfo {
                index: index as u8,
                public_key,
                script_hash: UInt160::from_script(&verification),
            });
            private_keys.push(private);
        }

        (validators, private_keys)
    }

    #[allow(clippy::too_many_arguments)]
    fn signed_tx_with_fees(
        settings: &ProtocolSettings,
        private: &[u8; 32],
        public: &[u8],
        account: UInt160,
        nonce: u32,
        system_fee: i64,
        network_fee: i64,
        attributes: Vec<TransactionAttribute>,
    ) -> Transaction {
        let verification =
            neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(public);

        let mut tx = Transaction::new();
        tx.set_nonce(nonce);
        tx.set_system_fee(system_fee);
        tx.set_network_fee(network_fee);
        tx.set_valid_until_block(1);
        tx.set_script(vec![OpCode::PUSH1.byte()]);
        tx.set_attributes(attributes);
        tx.set_signers(vec![Signer::new(account, WitnessScope::NONE)]);

        let hash = tx.try_hash().expect("tx hash");
        let mut sign_data = settings.network.to_le_bytes().to_vec();
        sign_data.extend_from_slice(&hash.to_bytes());
        let signature = Secp256r1Crypto::sign(&sign_data, private).expect("sign");

        let mut invocation = vec![OpCode::PUSHDATA1.byte(), 64];
        invocation.extend_from_slice(&signature);
        tx.set_witnesses(vec![Witness::new_with_scripts(invocation, verification)]);
        tx
    }
}
