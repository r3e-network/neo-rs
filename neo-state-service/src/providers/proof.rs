//! State-proof verification facade.

use neo_crypto::mpt_trie::Trie;
use neo_primitives::UInt256;
use neo_storage::persistence::providers::memory_store::MemoryStore;

use crate::MptReadSnapshot;

use super::{StateProof, StateProviderResult};

/// Verifies `proof` for `key` against `root_hash` and returns the proven value.
///
/// Verification builds its own isolated proof store inside the MPT library; the
/// `MptReadSnapshot` type parameter only selects the statically dispatched trie
/// implementation and no node StateService storage is accessed.
pub fn verify_state_proof(
    root_hash: UInt256,
    key: &[u8],
    proof: &StateProof,
) -> StateProviderResult<Vec<u8>> {
    Trie::<MptReadSnapshot<MemoryStore>>::verify_proof(root_hash, key, proof).map_err(Into::into)
}
