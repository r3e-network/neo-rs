//! Capability contracts for frozen StateService reads.

use std::collections::HashSet;

use neo_primitives::UInt256;

use crate::StateRoot;

use super::StateProviderResult;

/// Merkle proof nodes returned for one state key.
pub type StateProof = HashSet<Vec<u8>>;

/// One raw Neo storage key/value pair returned by a state prefix scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateEntry {
    /// Full C#-compatible storage key bytes, including the contract id.
    pub key: Vec<u8>,
    /// Raw storage-item value bytes.
    pub value: Vec<u8>,
}

/// Read-only state capability frozen at one MPT root.
///
/// Methods take `&mut self` because trie resolution maintains a request-local
/// node cache. This mutability never changes persisted state or the frozen
/// snapshot observed by the view.
pub trait StateView {
    /// Returns the root selected when this view was created.
    fn root_hash(&self) -> UInt256;

    /// Returns the block height used to select this view, when it was opened
    /// through [`StateProviderFactory::latest`] or
    /// [`StateProviderFactory::state_at`]. Root-addressed views return `None`.
    fn block_index(&self) -> Option<u32>;

    /// Returns the raw value stored under `key` at this view's root.
    fn get(&mut self, key: &[u8]) -> StateProviderResult<Option<Vec<u8>>>;

    /// Enumerates at most `limit` entries under `prefix`, resuming strictly
    /// after `from` when supplied.
    fn find(
        &mut self,
        prefix: &[u8],
        from: Option<&[u8]>,
        limit: usize,
    ) -> StateProviderResult<Vec<StateEntry>>;

    /// Builds a Merkle proof for `key`, or returns `None` when the key is
    /// absent at this view's root.
    fn proof(&mut self, key: &[u8]) -> StateProviderResult<Option<StateProof>>;
}

/// Factory for immutable state views.
///
/// The associated provider keeps state-query hot paths monomorphized while the
/// factory centralizes root selection, pruning-mode gates, and snapshot
/// isolation.
pub trait StateProviderFactory {
    /// Concrete view returned by this factory.
    type Provider: StateView;

    /// Creates a view at the latest locally persisted StateService root.
    fn latest(&self) -> StateProviderResult<Option<Self::Provider>>;

    /// Returns the latest local state-root record without requiring that its
    /// historical trie remain queryable.
    fn latest_root(&self) -> StateProviderResult<Option<StateRoot>>;

    /// Creates a view at the state root recorded for `height`.
    fn state_at(&self, height: u32) -> StateProviderResult<Option<Self::Provider>>;

    /// Returns the local state-root record for `height`.
    ///
    /// Root metadata remains available in pruning mode even when superseded MPT
    /// nodes no longer permit a [`StateView`] at that height.
    fn root_at(&self, height: u32) -> StateProviderResult<Option<StateRoot>>;

    /// Creates a view at `root_hash`.
    ///
    /// In pruned mode, only the current root is accepted. Full-history mode
    /// accepts any root and lets the first trie operation report an unknown or
    /// corrupt root through the normal provider error path.
    fn state_by_root(&self, root_hash: UInt256) -> StateProviderResult<Self::Provider>;
}
