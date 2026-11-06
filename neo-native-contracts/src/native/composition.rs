//! Standard native composition; the engine only consumes the resulting ports.

use super::{
    ContractManagement, CryptoLib, GasToken, LedgerContract, NativeContract, NeoToken, Notary,
    OracleContract, PolicyContract, RoleManagement, StdLib, TreasuryContract,
};
use neo_primitives::{UInt160, UInt256};
use neo_protocol::payloads::{Block, Extensible, Header, Transaction};
use neo_protocol::signer::Signer;
use neo_smart_contract::application_engine::{
    EngineNativeServices, NativeRegistry, NativeStateReader, WitnessHashResolver,
};
use neo_smart_contract::contract_state::ContractState;
use neo_smart_contract::error::CoreResult;
use neo_storage::persistence::DataCache;
use std::sync::{Arc, OnceLock};

/// Registers standard contracts with a fresh ContractManagement for every engine.
pub fn standard_registry() -> NativeRegistry {
    let mut registry = NativeRegistry::empty();
    registry.register(Arc::new(ContractManagement::new()));
    for contract in shared_stateless_native_contracts() {
        registry.register(Arc::clone(contract));
    }
    registry
}

/// Composes native services without depending on a ledger resolver implementation.
pub fn standard_native_services(
    witness_hash_resolver: Arc<dyn WitnessHashResolver>,
) -> EngineNativeServices {
    EngineNativeServices {
        registry: standard_registry(),
        state_reader: Arc::new(StandardNativeStateReader),
        witness_hash_resolver,
        policy_hash: PolicyContract::new().hash(),
        default_exec_fee_factor: PolicyContract::DEFAULT_EXEC_FEE_FACTOR,
        default_storage_price: PolicyContract::DEFAULT_STORAGE_PRICE,
    }
}

/// Standard native services wired with the shared verifying-hash resolver.
///
/// This is the canonical construction used by every verification/wallet engine
/// that needs the full standard native registry. It mirrors
/// `neo_native_contracts::native::composition::default_native_services`,
/// but binds the resolver to `neo_protocol::payloads` types so this crate stays
/// independent of `neo-core`'s own payload definitions. A single shared
/// instance is reused by all native-contract tests instead of per-call stubs.
pub fn default_native_services() -> EngineNativeServices {
    standard_native_services(Arc::new(StandardWitnessHashResolver))
}

/// Production [`WitnessHashResolver`] that resolves verifying script hashes by
/// downcasting the container to the concrete `neo_protocol::payloads` types.
///
/// This reproduces the downcast semantics of `neo-core`'s
/// `StandardWitnessHashResolver` (transaction / block / header / extensible),
/// expressed over the protocol crate's payload types so the resolver can live
/// in the decomposed native-contract layer without a `neo-core` dependency.
pub struct StandardWitnessHashResolver;

impl WitnessHashResolver for StandardWitnessHashResolver {
    fn resolve(
        &self,
        verifiable: &dyn neo_primitives::Verifiable,
        snapshot: &DataCache,
    ) -> Vec<UInt160> {
        if let Some(transaction) = verifiable.as_any().downcast_ref::<Transaction>() {
            return transaction.signer_accounts();
        }
        if let Some(block) = verifiable.as_any().downcast_ref::<Block>() {
            return header_script_hashes(&block.header, snapshot);
        }
        if let Some(header) = verifiable.as_any().downcast_ref::<Header>() {
            return header_script_hashes(header, snapshot);
        }
        if let Some(extensible) = verifiable.as_any().downcast_ref::<Extensible>() {
            return vec![extensible.sender];
        }
        Vec::new()
    }
}

/// Resolves the verifying script hashes for a payload header: the genesis
/// header (zero previous hash) verifies against its own witness script hash,
/// while later headers verify against the previous block's `next_consensus`.
/// This matches the ledger-owned resolver semantics preserved from `neo-core`.
fn header_script_hashes(header: &Header, snapshot: &DataCache) -> Vec<UInt160> {
    if *header.prev_hash() == UInt256::default() {
        return vec![header.witness.script_hash()];
    }
    match LedgerContract::new().get_trimmed_block(snapshot, header.prev_hash()) {
        Ok(Some(previous)) => vec![previous.header.next_consensus],
        Ok(None) => {
            tracing::warn!(
                prev_hash = %header.prev_hash(),
                "previous header not found when verifying header"
            );
            Vec::new()
        }
        Err(err) => {
            tracing::warn!(
                prev_hash = %header.prev_hash(),
                error = %err,
                "failed to fetch previous header when verifying header"
            );
            Vec::new()
        }
    }
}

/// Native-backed reads that preserve the original snapshot and error semantics.
pub struct StandardNativeStateReader;

impl NativeStateReader for StandardNativeStateReader {
    fn contract(&self, snapshot: &DataCache, hash: &UInt160) -> CoreResult<Option<ContractState>> {
        ContractManagement::get_contract_from_snapshot(snapshot, hash)
    }

    fn current_index(&self, snapshot: &DataCache) -> CoreResult<u32> {
        LedgerContract::new().current_index(snapshot)
    }

    fn is_blocked(&self, snapshot: &DataCache, hash: &UInt160) -> CoreResult<bool> {
        PolicyContract::new().is_blocked_snapshot(snapshot, hash)
    }

    fn whitelisted_fee(
        &self,
        snapshot: &DataCache,
        hash: &UInt160,
        method: &str,
        param_count: u32,
    ) -> CoreResult<Option<i64>> {
        PolicyContract::new().get_whitelisted_fee(snapshot, hash, method, param_count)
    }

    fn oracle_original_signers(
        &self,
        snapshot: &DataCache,
        id: u64,
    ) -> CoreResult<Option<Vec<Signer>>> {
        let Some(request) = OracleContract::new().get_request(snapshot, id)? else {
            return Ok(None);
        };
        Ok(LedgerContract::new()
            .get_transaction_state(snapshot, &request.original_tx_id)?
            .map(|state| state.transaction().signers().to_vec()))
    }
}

/// Process-wide immutable native contracts, in C# `NativeContract.Contracts`
/// order (excluding the stateful `ContractManagement`).
fn shared_stateless_native_contracts() -> &'static [Arc<dyn NativeContract>] {
    static CONTRACTS: OnceLock<Vec<Arc<dyn NativeContract>>> = OnceLock::new();
    CONTRACTS.get_or_init(|| {
        vec![
            Arc::new(StdLib::new()),
            Arc::new(CryptoLib::new()),
            Arc::new(LedgerContract::new()),
            Arc::new(NeoToken::new()),
            Arc::new(GasToken::new()),
            Arc::new(PolicyContract::new()),
            Arc::new(RoleManagement::new()),
            Arc::new(OracleContract::new()),
            Arc::new(Notary::new()),
            Arc::new(TreasuryContract::new()),
        ]
    })
}
