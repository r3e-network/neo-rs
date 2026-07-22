use neo_config::NeoChainSpec;
use neo_error::{CoreError, CoreResult};
use neo_payloads::{Block, Header, Witness};
use neo_primitives::{UInt160, UInt256};
use neo_storage::persistence::SeekDirection;
use neo_storage::{DataCache, StorageKey};

/// `LedgerContract` native id (a fixed protocol constant, C# id -4).
/// Hardcoded because the blockchain crate reaches natives only through
/// the type-erased provider seam; pinned against the real constant by
/// a dev-dependency test.
pub(crate) const LEDGER_CONTRACT_ID: i32 = -4;
/// C# `LedgerContract.Prefix_Block` (5): trimmed-block records by hash.
pub(crate) const LEDGER_PREFIX_BLOCK: u8 = 5;
/// `NeoToken` native id (a fixed protocol constant, C# id -5).
pub(crate) const NEO_TOKEN_ID: i32 = -5;
/// C# `NeoToken.Prefix_Committee` (14): the cached-committee record --
/// the first key genesis initialization writes.
pub(crate) const NEO_PREFIX_COMMITTEE_KEY: u8 = 14;

/// C# `NativeContract.Ledger.Initialized(snapshot)` (LedgerContract.cs:91):
/// whether the chain state has been bootstrapped, i.e. the genesis block
/// has been persisted. The point probe checks the `NeoToken` committee
/// cache -- a key genesis initialization always seeds and that can never be
/// deleted afterwards. This is the normal constant-time startup path. The
/// prefix probe retains the literal C# check (any `LedgerContract`
/// `Prefix_Block` record, written by the persist pipeline via
/// `crate::ledger_records`) for partially initialized legacy stores whose
/// ledger records landed without the native state.
pub fn chain_state_initialized<B: neo_storage::CacheRead>(snapshot: &DataCache<B>) -> bool {
    if snapshot
        .get(&StorageKey::new(
            NEO_TOKEN_ID,
            vec![NEO_PREFIX_COMMITTEE_KEY],
        ))
        .is_some()
    {
        return true;
    }
    let block_prefix = StorageKey::new(LEDGER_CONTRACT_ID, vec![LEDGER_PREFIX_BLOCK]);
    snapshot
        .find(Some(&block_prefix), SeekDirection::Forward)
        .next()
        .is_some()
}

/// C# `NeoSystem.CreateGenesisBlock(settings)`: index 0, zero
/// previous/merkle hashes, the chain specification's deterministic timestamp
/// and nonce, primary index 0, `NextConsensus` set to the BFT address of the
/// validated standby validators, and an empty-invocation `PUSH1` witness. The
/// genesis block carries no transactions.
///
/// Requiring the complete [`NeoChainSpec`] keeps genesis identity authoritative:
/// custom chains cannot accidentally combine their configured timestamp or
/// nonce with validators reconstructed from an unrelated settings object.
pub fn genesis_block(chain_spec: &NeoChainSpec) -> CoreResult<Block> {
    let genesis = chain_spec.genesis();
    let settings = chain_spec.protocol_settings();
    let mut header = Header::new();
    header.set_version(0);
    header.set_prev_hash(UInt256::zero());
    header.set_merkle_root(UInt256::zero());
    header.set_timestamp(genesis.timestamp);
    header.set_nonce(genesis.nonce);
    header.set_index(0);
    header.set_primary_index(0);
    header.set_next_consensus(bft_address(&settings.standby_validators())?);
    header.witness = Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH1.byte()]);
    Ok(Block::from_parts(header, Vec::new()))
}

/// C# `Contract.GetBFTAddress(pubkeys)`: the `m = n - (n - 1) / 3` multisig
/// script hash. Delegates to the single workspace implementation.
pub(crate) fn bft_address(pubkeys: &[neo_crypto::ECPoint]) -> CoreResult<UInt160> {
    neo_vm::script_builder::RedeemScript::bft_address(pubkeys)
        .ok_or_else(|| CoreError::invalid_operation("BFT address requires at least one validator"))
}
