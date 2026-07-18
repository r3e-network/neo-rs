use super::bounds::{ExecutionArtifactError, ExecutionArtifactLimits};
use neo_vm::{InteropInterface, Script, StackItem};
use std::collections::HashMap;
use std::sync::Arc;

/// Canonical reference or primitive stack value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CanonicalStackValue {
    /// NeoVM Null.
    Null,
    /// NeoVM Boolean.
    Boolean(bool),
    /// Minimal signed little-endian NeoVM integer bytes.
    Integer(Vec<u8>),
    /// Immutable bytes.
    ByteString(Vec<u8>),
    /// Reference to an identity-bearing canonical node.
    Reference(u32),
    /// Pointer position and reference to its originating script node.
    Pointer {
        /// Canonical script node.
        script: u32,
        /// Instruction position.
        position: u64,
    },
}

/// Canonical interop payload paired with normalized reference identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CanonicalInteropInterface {
    /// Engine-owned storage iterator handle.
    Iterator(u32),
    /// Canonical BLS12-381 payload bytes.
    Bls12381(Vec<u8>),
}

/// Identity-bearing node in a canonical stack graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CanonicalStackNode {
    /// Mutable Buffer, compared by normalized identity plus contents.
    Buffer(Vec<u8>),
    /// Mutable/read-only Array.
    Array {
        /// C# read-only flag.
        read_only: bool,
        /// Array elements in index order.
        items: Vec<CanonicalStackValue>,
    },
    /// Mutable/read-only Struct.
    Struct {
        /// C# read-only flag.
        read_only: bool,
        /// Struct fields in index order.
        items: Vec<CanonicalStackValue>,
    },
    /// Mutable/read-only ordered Map.
    Map {
        /// C# read-only flag.
        read_only: bool,
        /// Entries in exact VM dictionary order.
        entries: Vec<(CanonicalStackValue, CanonicalStackValue)>,
    },
    /// Originating script object used by one or more Pointer values.
    Script(Vec<u8>),
    /// Reference-valued interop interface.
    Interop(CanonicalInteropInterface),
}

/// Canonical graph node table shared by every stack-bearing artifact surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalStackGraph {
    nodes: Vec<CanonicalStackNode>,
}

impl CanonicalStackGraph {
    /// Returns canonical nodes in deterministic first-traversal order.
    #[must_use]
    pub fn nodes(&self) -> &[CanonicalStackNode] {
        &self.nodes
    }
}

/// Standalone canonical stack document used by graph-level differential tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalStackDocument {
    roots: Vec<CanonicalStackValue>,
    graph: CanonicalStackGraph,
    usage: CanonicalStackUsage,
}

impl CanonicalStackDocument {
    /// Canonicalizes stack roots while preserving cross-root alias topology.
    pub fn capture(
        roots: &[StackItem],
        limits: ExecutionArtifactLimits,
    ) -> Result<Self, ExecutionArtifactError> {
        Self::capture_segments(std::iter::once(roots), limits)
    }

    pub(super) fn capture_segments<'a>(
        segments: impl IntoIterator<Item = &'a [StackItem]>,
        limits: ExecutionArtifactLimits,
    ) -> Result<Self, ExecutionArtifactError> {
        let mut builder = CanonicalGraphBuilder::new(limits);
        let mut roots = Vec::new();
        for segment in segments {
            roots.extend(builder.roots(segment)?);
        }
        let usage = builder.usage();
        let graph = builder.finish();
        Ok(Self {
            roots,
            graph,
            usage,
        })
    }

    /// Returns roots in caller-supplied order.
    #[must_use]
    pub fn roots(&self) -> &[CanonicalStackValue] {
        &self.roots
    }

    /// Returns the normalized identity graph.
    #[must_use]
    pub const fn graph(&self) -> &CanonicalStackGraph {
        &self.graph
    }

    pub(super) const fn usage(&self) -> CanonicalStackUsage {
        self.usage
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CanonicalStackUsage {
    pub(super) roots: usize,
    pub(super) nodes: usize,
    pub(super) edges: usize,
    pub(super) max_depth: usize,
    pub(super) retained_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum RuntimeIdentity {
    Buffer(usize),
    Array(usize),
    Struct(usize),
    Map(usize),
    Script(usize),
    Interop(usize),
}

pub(super) struct CanonicalGraphBuilder {
    pub(super) limits: ExecutionArtifactLimits,
    identities: HashMap<RuntimeIdentity, u32>,
    nodes: Vec<Option<CanonicalStackNode>>,
    embedded_nodes: usize,
    roots: usize,
    edges: usize,
    max_depth: usize,
    retained_bytes: usize,
}

impl CanonicalGraphBuilder {
    pub(super) fn new(limits: ExecutionArtifactLimits) -> Self {
        Self {
            limits,
            identities: HashMap::new(),
            nodes: Vec::new(),
            embedded_nodes: 0,
            roots: 0,
            edges: 0,
            max_depth: 0,
            retained_bytes: 0,
        }
    }

    pub(super) fn require_count(
        actual: usize,
        maximum: usize,
        resource: &'static str,
    ) -> Result<(), ExecutionArtifactError> {
        if actual > maximum {
            return Err(ExecutionArtifactError::LimitExceeded {
                resource,
                actual,
                maximum,
            });
        }
        Ok(())
    }

    pub(super) fn retain_bytes(&mut self, bytes: usize) -> Result<(), ExecutionArtifactError> {
        let actual = self.retained_bytes.checked_add(bytes).ok_or(
            ExecutionArtifactError::LimitExceeded {
                resource: "retained bytes",
                actual: usize::MAX,
                maximum: self.limits.max_retained_bytes,
            },
        )?;
        Self::require_count(actual, self.limits.max_retained_bytes, "retained bytes")?;
        self.retained_bytes = actual;
        Ok(())
    }

    pub(super) fn retain_document(
        &mut self,
        document: &CanonicalStackDocument,
    ) -> Result<(), ExecutionArtifactError> {
        let usage = document.usage();
        let roots = self.roots.saturating_add(usage.roots);
        let nodes = self
            .nodes
            .len()
            .saturating_add(self.embedded_nodes)
            .saturating_add(usage.nodes);
        let edges = self.edges.saturating_add(usage.edges);
        let retained_bytes = self
            .retained_bytes
            .checked_add(usage.retained_bytes)
            .ok_or(ExecutionArtifactError::LimitExceeded {
                resource: "retained bytes",
                actual: usize::MAX,
                maximum: self.limits.max_retained_bytes,
            })?;

        Self::require_count(roots, self.limits.max_stack_roots, "stack roots")?;
        Self::require_count(nodes, self.limits.max_stack_nodes, "stack graph nodes")?;
        Self::require_count(edges, self.limits.max_stack_edges, "stack graph edges")?;
        Self::require_count(
            usage.max_depth,
            self.limits.max_stack_depth,
            "stack graph depth",
        )?;
        Self::require_count(
            retained_bytes,
            self.limits.max_retained_bytes,
            "retained bytes",
        )?;

        self.roots = roots;
        self.embedded_nodes = self.embedded_nodes.saturating_add(usage.nodes);
        self.edges = edges;
        self.max_depth = self.max_depth.max(usage.max_depth);
        self.retained_bytes = retained_bytes;
        Ok(())
    }

    fn add_edges(&mut self, count: usize) -> Result<(), ExecutionArtifactError> {
        let actual = self.edges.saturating_add(count);
        Self::require_count(actual, self.limits.max_stack_edges, "stack graph edges")?;
        self.edges = actual;
        Ok(())
    }

    pub(super) fn roots(
        &mut self,
        values: &[StackItem],
    ) -> Result<Vec<CanonicalStackValue>, ExecutionArtifactError> {
        let actual = self.roots.saturating_add(values.len());
        Self::require_count(actual, self.limits.max_stack_roots, "stack roots")?;
        self.roots = actual;
        values.iter().map(|value| self.value(value, 0)).collect()
    }

    pub(super) fn optional_root(
        &mut self,
        value: Option<&StackItem>,
    ) -> Result<Option<CanonicalStackValue>, ExecutionArtifactError> {
        value
            .map(|value| {
                self.roots(std::slice::from_ref(value))
                    .map(|mut roots| roots.remove(0))
            })
            .transpose()
    }

    fn reserve_node(
        &mut self,
        identity: RuntimeIdentity,
    ) -> Result<(u32, bool), ExecutionArtifactError> {
        if let Some(id) = self.identities.get(&identity) {
            return Ok((*id, false));
        }
        let actual = self
            .nodes
            .len()
            .saturating_add(self.embedded_nodes)
            .saturating_add(1);
        Self::require_count(actual, self.limits.max_stack_nodes, "stack graph nodes")?;
        let id = u32::try_from(self.nodes.len()).map_err(|_| {
            ExecutionArtifactError::NumericOverflow {
                field: "stack node id",
            }
        })?;
        self.identities.insert(identity, id);
        self.nodes.push(None);
        Ok((id, true))
    }

    fn fill_node(&mut self, id: u32, node: CanonicalStackNode) {
        self.nodes[id as usize] = Some(node);
    }

    fn value(
        &mut self,
        value: &StackItem,
        depth: usize,
    ) -> Result<CanonicalStackValue, ExecutionArtifactError> {
        Self::require_count(depth, self.limits.max_stack_depth, "stack graph depth")?;
        self.max_depth = self.max_depth.max(depth);
        match value {
            StackItem::Null => Ok(CanonicalStackValue::Null),
            StackItem::Boolean(value) => Ok(CanonicalStackValue::Boolean(*value)),
            StackItem::Integer(value) => {
                let bytes = value.to_signed_bytes_le();
                self.retain_bytes(bytes.len())?;
                Ok(CanonicalStackValue::Integer(bytes))
            }
            StackItem::ByteString(bytes) => {
                self.retain_bytes(bytes.len())?;
                Ok(CanonicalStackValue::ByteString(bytes.clone()))
            }
            StackItem::Buffer(buffer) => {
                let (id, fresh) = self.reserve_node(RuntimeIdentity::Buffer(buffer.id()))?;
                if fresh {
                    let bytes = buffer.data();
                    self.retain_bytes(bytes.len())?;
                    self.fill_node(id, CanonicalStackNode::Buffer(bytes));
                }
                Ok(CanonicalStackValue::Reference(id))
            }
            StackItem::Array(array) => {
                let (id, fresh) = self.reserve_node(RuntimeIdentity::Array(array.id()))?;
                if fresh {
                    let values = array.items();
                    self.add_edges(values.len())?;
                    let items = values
                        .iter()
                        .map(|value| self.value(value, depth.saturating_add(1)))
                        .collect::<Result<Vec<_>, _>>()?;
                    self.fill_node(
                        id,
                        CanonicalStackNode::Array {
                            read_only: array.is_read_only(),
                            items,
                        },
                    );
                }
                Ok(CanonicalStackValue::Reference(id))
            }
            StackItem::Struct(structure) => {
                let (id, fresh) = self.reserve_node(RuntimeIdentity::Struct(structure.id()))?;
                if fresh {
                    let values = structure.items();
                    self.add_edges(values.len())?;
                    let items = values
                        .iter()
                        .map(|value| self.value(value, depth.saturating_add(1)))
                        .collect::<Result<Vec<_>, _>>()?;
                    self.fill_node(
                        id,
                        CanonicalStackNode::Struct {
                            read_only: structure.is_read_only(),
                            items,
                        },
                    );
                }
                Ok(CanonicalStackValue::Reference(id))
            }
            StackItem::Map(map) => {
                let (id, fresh) = self.reserve_node(RuntimeIdentity::Map(map.id()))?;
                if fresh {
                    let values = map.items();
                    self.add_edges(values.len().saturating_mul(2))?;
                    let mut entries = Vec::with_capacity(values.len());
                    for (key, value) in values.iter() {
                        entries.push((
                            self.value(key, depth.saturating_add(1))?,
                            self.value(value, depth.saturating_add(1))?,
                        ));
                    }
                    self.fill_node(
                        id,
                        CanonicalStackNode::Map {
                            read_only: map.is_read_only(),
                            entries,
                        },
                    );
                }
                Ok(CanonicalStackValue::Reference(id))
            }
            StackItem::Pointer(pointer) => {
                self.add_edges(1)?;
                let script = pointer.script_arc();
                let script_id = self.script_node(&script)?;
                let position = u64::try_from(pointer.position()).map_err(|_| {
                    ExecutionArtifactError::NumericOverflow {
                        field: "pointer position",
                    }
                })?;
                Ok(CanonicalStackValue::Pointer {
                    script: script_id,
                    position,
                })
            }
            StackItem::InteropInterface(interface) => {
                let identity = RuntimeIdentity::Interop(Arc::as_ptr(interface) as usize);
                let (id, fresh) = self.reserve_node(identity)?;
                if fresh {
                    let canonical = match interface.as_ref() {
                        InteropInterface::Iterator { id } => {
                            CanonicalInteropInterface::Iterator(*id)
                        }
                        InteropInterface::Bls12381 { bytes } => {
                            self.retain_bytes(bytes.len())?;
                            CanonicalInteropInterface::Bls12381(bytes.clone())
                        }
                    };
                    self.fill_node(id, CanonicalStackNode::Interop(canonical));
                }
                Ok(CanonicalStackValue::Reference(id))
            }
        }
    }

    pub(super) fn script_node(
        &mut self,
        script: &Arc<Script>,
    ) -> Result<u32, ExecutionArtifactError> {
        let identity = RuntimeIdentity::Script(Arc::as_ptr(script) as usize);
        let (id, fresh) = self.reserve_node(identity)?;
        if fresh {
            self.retain_bytes(script.as_bytes().len())?;
            self.fill_node(id, CanonicalStackNode::Script(script.to_array()));
        }
        Ok(id)
    }

    pub(super) fn finish(self) -> CanonicalStackGraph {
        let nodes = self
            .nodes
            .into_iter()
            .map(|node| node.expect("canonical graph nodes are filled before capture completes"))
            .collect();
        CanonicalStackGraph { nodes }
    }

    fn usage(&self) -> CanonicalStackUsage {
        CanonicalStackUsage {
            roots: self.roots,
            nodes: self.nodes.len().saturating_add(self.embedded_nodes),
            edges: self.edges,
            max_depth: self.max_depth,
            retained_bytes: self.retained_bytes,
        }
    }
}
