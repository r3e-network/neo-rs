//! # neo-node::neo_pack_common
//!
//! Shared composition helpers for offline authoritative state-pack tools.
//!
//! ## Boundary
//!
//! This module adapts a pinned `neo-state-packs` snapshot to the canonical MPT
//! read capability. Pack formats remain opaque to `neo-crypto`, while pack
//! tools share one bounded root-graph validation path.
//!
//! ## Contents
//!
//! - [`validate_pack_root_graph`]: validate one current StateRoot through a
//!   pinned and value-bounded pack snapshot.

use anyhow::{Context, Result};
use neo_primitives::UInt256;
use neo_state_packs::{PACK_KEY_BYTES, PackStore, Snapshot};
use neo_trie::{
    MPT_NODE_PREFIX, MptError, MptResult, MptStoreSnapshot, PersistedMptGraphLimits,
    PersistedMptGraphReport, validate_persisted_root_graph,
};

pub(crate) const DEFAULT_MAX_ROOT_GRAPH_NODES: u64 = 64_000_000;
pub(crate) const DEFAULT_MAX_ROOT_GRAPH_BYTES: u64 = 64 * 1024 * 1024 * 1024;
pub(crate) const MAX_MPT_NODE_BYTES: u64 = 1024 * 1024;

struct BoundedPackMptSnapshot {
    snapshot: Snapshot,
    max_node_bytes: u64,
}

impl MptStoreSnapshot for BoundedPackMptSnapshot {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        let key: &[u8; PACK_KEY_BYTES] = key
            .try_into()
            .map_err(|_| MptError::key("MPT pack lookup key must contain exactly 33 bytes"))?;
        if key[0] != MPT_NODE_PREFIX {
            return Err(MptError::key(
                "MPT pack lookup key is outside the canonical node namespace",
            ));
        }
        self.snapshot
            .get_bounded(key, self.max_node_bytes)
            .map_err(|error| MptError::storage(error.to_string()))
    }

    fn put(&self, _key: Vec<u8>, _value: Vec<u8>) -> MptResult<()> {
        Err(MptError::invalid(
            "pinned state-pack snapshots are read-only",
        ))
    }

    fn delete(&self, _key: Vec<u8>) -> MptResult<()> {
        Err(MptError::invalid(
            "pinned state-pack snapshots are read-only",
        ))
    }
}

pub(crate) fn validate_pack_root_graph(
    pack: &PackStore,
    root_internal: [u8; 32],
    limits: PersistedMptGraphLimits,
) -> Result<PersistedMptGraphReport> {
    let root = UInt256::from_bytes(&root_internal).context("decode internal StateRoot bytes")?;
    let snapshot = pack.snapshot().context("pin checkpoint pack snapshot")?;
    let snapshot = BoundedPackMptSnapshot {
        snapshot,
        max_node_bytes: limits.max_node_bytes,
    };
    validate_persisted_root_graph(&snapshot, root, limits)
        .map_err(|error| anyhow::anyhow!("validate checkpoint StateRoot graph: {error}"))
}
