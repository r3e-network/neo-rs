//! # neo-trie::root_validation
//!
//! Bounded validation of the materialized nodes reachable from one persisted
//! Neo MPT root.
//!
//! ## Boundary
//!
//! This module owns Neo MPT graph shape, content-address binding, and traversal
//! limits. It reads through [`MptStoreSnapshot`] and does not know which
//! database, pack, checkpoint, or node service supplies that frozen view.
//!
//! ## Contents
//!
//! - [`PersistedMptGraphLimits`]: explicit traversal and byte ceilings.
//! - [`PersistedMptGraphReport`]: deterministic current-root evidence.
//! - [`validate_persisted_root_graph`]: bounded graph validation entry point.

use super::cache::node_key_bytes;
use super::{MPT_NODE_PREFIX, MptError, MptResult, MptStoreSnapshot, Node, NodeType};
use neo_primitives::UInt256;
use rustc_hash::FxHashMap;

/// Resource ceilings for one persisted current-root graph validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PersistedMptGraphLimits {
    /// Maximum number of distinct content-addressed nodes that may be visited.
    pub max_nodes: u64,
    /// Maximum sum of serialized bytes across all distinct visited nodes.
    pub max_total_bytes: u64,
    /// Maximum serialized size accepted for any one node row.
    pub max_node_bytes: u64,
}

impl PersistedMptGraphLimits {
    /// Creates explicit limits for one validation run.
    #[must_use]
    pub const fn new(max_nodes: u64, max_total_bytes: u64, max_node_bytes: u64) -> Self {
        Self {
            max_nodes,
            max_total_bytes,
            max_node_bytes,
        }
    }

    fn validate(self) -> MptResult<Self> {
        if self.max_nodes == 0 {
            return Err(MptError::invalid(
                "persisted MPT graph max_nodes must be greater than zero",
            ));
        }
        if self.max_total_bytes == 0 {
            return Err(MptError::invalid(
                "persisted MPT graph max_total_bytes must be greater than zero",
            ));
        }
        if self.max_node_bytes == 0 {
            return Err(MptError::invalid(
                "persisted MPT graph max_node_bytes must be greater than zero",
            ));
        }
        if self.max_node_bytes > self.max_total_bytes {
            return Err(MptError::invalid(
                "persisted MPT graph max_node_bytes cannot exceed max_total_bytes",
            ));
        }
        Ok(self)
    }
}

/// Deterministic evidence collected while validating one persisted root graph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PersistedMptGraphReport {
    /// Number of distinct content-addressed nodes reachable from the root.
    pub unique_nodes: u64,
    /// Sum of exact serialized row bytes for all distinct reachable nodes.
    pub total_bytes: u64,
    /// Number of reachable branch nodes.
    pub branch_nodes: u64,
    /// Number of reachable extension nodes.
    pub extension_nodes: u64,
    /// Number of reachable leaf nodes.
    pub leaf_nodes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VisitState {
    Active,
    Complete,
}

enum TraversalStep {
    Enter(UInt256),
    Children {
        parent: UInt256,
        children: Vec<UInt256>,
        next: usize,
    },
}

/// Validates every persisted node reachable from `root` under explicit bounds.
///
/// Rows are loaded from the canonical `0xf0 || node_hash` namespace. Every
/// distinct row is decoded with [`Node::deserialize_persisted`], which binds
/// its materialized node payload to the requested hash. Branch children are
/// visited in index order and extension children are visited once. Shared
/// subgraphs count once; a back-edge to an active ancestor is rejected.
///
/// # Errors
///
/// Returns an error when a limit is zero or internally inconsistent, a row is
/// missing or malformed, a row hash does not match its key, a cycle is found,
/// or any node/count/byte limit would be exceeded.
pub fn validate_persisted_root_graph<S>(
    snapshot: &S,
    root: UInt256,
    limits: PersistedMptGraphLimits,
) -> MptResult<PersistedMptGraphReport>
where
    S: MptStoreSnapshot + ?Sized,
{
    let limits = limits.validate()?;
    validate_graph(root, limits, |hash, remaining_bytes| {
        let key = node_key_bytes(MPT_NODE_PREFIX, &hash);
        let bytes = snapshot.try_get(&key)?.ok_or_else(|| {
            MptError::storage(format!("persisted MPT graph is missing node {hash}"))
        })?;
        let byte_len = u64::try_from(bytes.len())
            .map_err(|_| MptError::invalid("persisted MPT node length does not fit u64"))?;
        if byte_len > limits.max_node_bytes {
            return Err(MptError::invalid(format!(
                "persisted MPT node {hash} has {byte_len} bytes, exceeding max_node_bytes {}",
                limits.max_node_bytes
            )));
        }
        if byte_len > remaining_bytes {
            return Err(MptError::invalid(format!(
                "persisted MPT graph exceeds max_total_bytes {} at node {hash}",
                limits.max_total_bytes
            )));
        }
        let node = Node::deserialize_persisted(&bytes, hash)?;
        Ok((node, byte_len))
    })
}

fn validate_graph<F>(
    root: UInt256,
    limits: PersistedMptGraphLimits,
    mut load: F,
) -> MptResult<PersistedMptGraphReport>
where
    F: FnMut(UInt256, u64) -> MptResult<(Node, u64)>,
{
    let mut states = FxHashMap::default();
    let mut steps = vec![TraversalStep::Enter(root)];
    let mut report = PersistedMptGraphReport::default();

    while let Some(step) = steps.pop() {
        match step {
            TraversalStep::Enter(hash) => {
                match states.get(&hash) {
                    Some(VisitState::Active) => return Err(cycle_error(hash)),
                    Some(VisitState::Complete) => continue,
                    None => {}
                }
                if report.unique_nodes >= limits.max_nodes {
                    return Err(MptError::invalid(format!(
                        "persisted MPT graph exceeds max_nodes {} before node {hash}",
                        limits.max_nodes
                    )));
                }

                states.insert(hash, VisitState::Active);
                let remaining_bytes = limits.max_total_bytes - report.total_bytes;
                let (node, byte_len) = load(hash, remaining_bytes)?;
                let next_total = report.total_bytes.checked_add(byte_len).ok_or_else(|| {
                    MptError::invalid("persisted MPT graph total byte count overflowed u64")
                })?;
                if next_total > limits.max_total_bytes {
                    return Err(MptError::invalid(format!(
                        "persisted MPT graph exceeds max_total_bytes {} at node {hash}",
                        limits.max_total_bytes
                    )));
                }

                report.unique_nodes += 1;
                report.total_bytes = next_total;
                report.record(node.node_type)?;
                let children = child_hashes(&node)?;
                steps.push(TraversalStep::Children {
                    parent: hash,
                    children,
                    next: 0,
                });
            }
            TraversalStep::Children {
                parent,
                children,
                next,
            } => {
                let Some(&child) = children.get(next) else {
                    match states.get_mut(&parent) {
                        Some(state @ VisitState::Active) => *state = VisitState::Complete,
                        Some(VisitState::Complete) | None => {
                            return Err(MptError::invalid(
                                "persisted MPT traversal state became inconsistent",
                            ));
                        }
                    }
                    continue;
                };

                steps.push(TraversalStep::Children {
                    parent,
                    children,
                    next: next + 1,
                });
                match states.get(&child) {
                    Some(VisitState::Active) => return Err(cycle_error(child)),
                    Some(VisitState::Complete) => {}
                    None => steps.push(TraversalStep::Enter(child)),
                }
            }
        }
    }

    Ok(report)
}

impl PersistedMptGraphReport {
    fn record(&mut self, node_type: NodeType) -> MptResult<()> {
        let count = match node_type {
            NodeType::BranchNode => &mut self.branch_nodes,
            NodeType::ExtensionNode => &mut self.extension_nodes,
            NodeType::LeafNode => &mut self.leaf_nodes,
            NodeType::HashNode | NodeType::Empty => {
                return Err(MptError::invalid(
                    "persisted MPT graph loader returned a non-materialized node",
                ));
            }
        };
        *count = count
            .checked_add(1)
            .ok_or_else(|| MptError::invalid("persisted MPT node-kind count overflowed u64"))?;
        Ok(())
    }
}

fn child_hashes(node: &Node) -> MptResult<Vec<UInt256>> {
    match node.node_type {
        NodeType::BranchNode => node.children.iter().try_fold(
            Vec::with_capacity(node.children.len()),
            |mut hashes, child| {
                match child.node_type {
                    NodeType::HashNode => hashes.push(child.try_hash()?),
                    NodeType::Empty => {}
                    NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => {
                        return Err(MptError::invalid(
                            "persisted MPT branch contains a materialized child",
                        ));
                    }
                }
                Ok(hashes)
            },
        ),
        NodeType::ExtensionNode => {
            let child = node
                .next
                .as_ref()
                .ok_or_else(|| MptError::invalid("persisted MPT extension is missing its child"))?;
            match child.node_type {
                NodeType::HashNode => Ok(vec![child.try_hash()?]),
                NodeType::Empty => Ok(Vec::new()),
                NodeType::BranchNode | NodeType::ExtensionNode | NodeType::LeafNode => Err(
                    MptError::invalid("persisted MPT extension contains a materialized child"),
                ),
            }
        }
        NodeType::LeafNode => Ok(Vec::new()),
        NodeType::HashNode | NodeType::Empty => Err(MptError::invalid(
            "persisted MPT graph contains a non-materialized row",
        )),
    }
}

fn cycle_error(hash: UInt256) -> MptError {
    MptError::invalid(format!(
        "persisted MPT graph contains a cycle through node {hash}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::UINT256_SIZE;

    #[test]
    fn traversal_core_rejects_an_active_path_cycle() {
        let first = UInt256::from([0x11; UINT256_SIZE]);
        let second = UInt256::from([0x22; UINT256_SIZE]);
        let limits = PersistedMptGraphLimits::new(2, 2, 1);

        let error = validate_graph(first, limits, |hash, _remaining_bytes| {
            let child = if hash == first { second } else { first };
            let mut branch = Node::new_branch();
            branch.set_child(0, Node::new_hash(child));
            Ok((branch, 1))
        })
        .expect_err("active-path back-edges must be rejected");

        assert!(error.to_string().contains("cycle"));
    }
}
