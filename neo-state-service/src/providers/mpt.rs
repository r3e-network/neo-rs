//! State provider backed by a frozen StateService MPT snapshot.

use std::sync::Arc;

use neo_primitives::UInt256;
use neo_storage::persistence::Store;
use neo_trie::Trie;

use crate::{MptReadSnapshot, MptStore};

use super::{StateEntry, StateProviderError, StateProviderFactory, StateProviderResult, StateView};

/// Request-scoped state view over one immutable MPT generation and root.
pub struct MptStateProvider<S>
where
    S: Store,
{
    root_hash: UInt256,
    block_index: Option<u32>,
    trie: Trie<MptReadSnapshot<S>>,
}

impl<S> MptStateProvider<S>
where
    S: Store,
{
    fn new(
        snapshot: Arc<MptReadSnapshot<S>>,
        root_hash: UInt256,
        block_index: Option<u32>,
    ) -> Self {
        let trie = snapshot.open_trie(Some(root_hash));
        Self {
            root_hash,
            block_index,
            trie,
        }
    }
}

impl<S> std::fmt::Debug for MptStateProvider<S>
where
    S: Store,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MptStateProvider")
            .field("root_hash", &self.root_hash)
            .field("block_index", &self.block_index)
            .finish_non_exhaustive()
    }
}

impl<S> StateView for MptStateProvider<S>
where
    S: Store,
{
    fn root_hash(&self) -> UInt256 {
        self.root_hash
    }

    fn block_index(&self) -> Option<u32> {
        self.block_index
    }

    fn get(&mut self, key: &[u8]) -> StateProviderResult<Option<Vec<u8>>> {
        self.trie.try_get_value(key).map_err(Into::into)
    }

    fn find(
        &mut self,
        prefix: &[u8],
        from: Option<&[u8]>,
        limit: usize,
    ) -> StateProviderResult<Vec<StateEntry>> {
        self.trie
            .find_limited(prefix, from, limit)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| StateEntry {
                        key: entry.key,
                        value: entry.value,
                    })
                    .collect()
            })
            .map_err(Into::into)
    }

    fn proof(&mut self, key: &[u8]) -> StateProviderResult<Option<super::StateProof>> {
        self.trie.try_get_proof(key).map_err(Into::into)
    }
}

/// Factory that freezes one [`MptReadSnapshot`] for every state view it opens.
#[derive(Clone, Debug)]
pub struct MptStateProviderFactory<S>
where
    S: Store,
{
    store: Arc<MptStore<S>>,
}

impl<S> MptStateProviderFactory<S>
where
    S: Store,
{
    /// Creates a factory over the shared persisted MPT store.
    #[must_use]
    pub const fn new(store: Arc<MptStore<S>>) -> Self {
        Self { store }
    }

    fn provider(
        snapshot: Arc<MptReadSnapshot<S>>,
        root_hash: UInt256,
        block_index: Option<u32>,
    ) -> StateProviderResult<MptStateProvider<S>> {
        let current_root = snapshot.current_local_root_hash();
        if !snapshot.full_state() && current_root != Some(root_hash) {
            return Err(StateProviderError::unsupported_state(
                false,
                current_root,
                root_hash,
            ));
        }
        Ok(MptStateProvider::new(snapshot, root_hash, block_index))
    }
}

impl<S> StateProviderFactory for MptStateProviderFactory<S>
where
    S: Store,
{
    type Provider = MptStateProvider<S>;

    fn latest(&self) -> StateProviderResult<Option<Self::Provider>> {
        let snapshot = self.store.snapshot();
        let Some(block_index) = snapshot.try_current_local_root_index()? else {
            return Ok(None);
        };
        let Some(state_root) = snapshot.try_get_state_root(block_index)? else {
            return Ok(None);
        };
        Self::provider(snapshot, *state_root.root_hash(), Some(block_index)).map(Some)
    }

    fn latest_root(&self) -> StateProviderResult<Option<crate::StateRoot>> {
        let snapshot = self.store.snapshot();
        let Some(block_index) = snapshot.try_current_local_root_index()? else {
            return Ok(None);
        };
        Ok(snapshot.try_get_state_root(block_index)?)
    }

    fn state_at(&self, height: u32) -> StateProviderResult<Option<Self::Provider>> {
        let snapshot = self.store.snapshot();
        let Some(state_root) = snapshot.try_get_state_root(height)? else {
            return Ok(None);
        };
        Self::provider(snapshot, *state_root.root_hash(), Some(height)).map(Some)
    }

    fn root_at(&self, height: u32) -> StateProviderResult<Option<crate::StateRoot>> {
        Ok(self.store.snapshot().try_get_state_root(height)?)
    }

    fn state_by_root(&self, root_hash: UInt256) -> StateProviderResult<Self::Provider> {
        Self::provider(self.store.snapshot(), root_hash, None)
    }
}

#[cfg(test)]
#[path = "../tests/providers/mpt.rs"]
mod tests;
