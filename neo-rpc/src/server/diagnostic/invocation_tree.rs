//! Invocation-tree state captured while diagnostic execution runs.

use neo_primitives::UInt160;

#[derive(Default)]
pub(super) struct InvocationTree {
    nodes: Vec<InvocationNode>,
    root: Option<usize>,
    current_node: Option<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct DiagnosticInvocation {
    pub(crate) hash: UInt160,
    pub(crate) children: Vec<DiagnosticInvocation>,
}

struct InvocationNode {
    hash: UInt160,
    parent: Option<usize>,
    children: Vec<usize>,
}

impl InvocationTree {
    pub(super) fn load_context(&mut self, hash: UInt160) {
        let next_node = match self.current_node {
            Some(parent) => self.add_child(parent, hash),
            None => self.add_root(hash),
        };
        self.current_node = Some(next_node);
    }

    pub(super) fn unload_context(&mut self) {
        self.current_node = self
            .current_node
            .and_then(|current| self.nodes.get(current).and_then(|node| node.parent));
    }

    pub(super) fn root_snapshot(&self) -> Option<DiagnosticInvocation> {
        self.root.map(|root| self.snapshot_node(root))
    }

    fn add_root(&mut self, hash: UInt160) -> usize {
        self.nodes.clear();
        self.nodes.push(InvocationNode {
            hash,
            parent: None,
            children: Vec::new(),
        });
        self.root = Some(0);
        0
    }

    fn add_child(&mut self, parent: usize, hash: UInt160) -> usize {
        let child = self.nodes.len();
        self.nodes.push(InvocationNode {
            hash,
            parent: Some(parent),
            children: Vec::new(),
        });
        self.nodes[parent].children.push(child);
        child
    }

    fn snapshot_node(&self, index: usize) -> DiagnosticInvocation {
        let node = &self.nodes[index];
        DiagnosticInvocation {
            hash: node.hash,
            children: node
                .children
                .iter()
                .map(|child| self.snapshot_node(*child))
                .collect(),
        }
    }
}
