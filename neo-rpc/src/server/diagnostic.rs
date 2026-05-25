use neo_core::smart_contract::diagnostic::IDiagnostic;
use neo_core::smart_contract::execution_context_state::ExecutionContextState;
use neo_core::smart_contract::ApplicationEngine;
use neo_core::vm_runtime::ExecutionContext;
use neo_core::UInt160;
use neo_vm_rs::Instruction;
use parking_lot::Mutex;
use std::fmt;
use std::sync::Arc;

/// Diagnostics helper mirroring `Neo.Plugins.RpcServer.Diagnostic`.
#[derive(Clone, Default)]
pub struct Diagnostic {
    inner: Arc<Mutex<DiagnosticState>>,
}

#[derive(Default)]
struct DiagnosticState {
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

impl DiagnosticState {
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

    fn root_snapshot(&self) -> Option<DiagnosticInvocation> {
        self.root.map(|root| self.snapshot_node(root))
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

impl Diagnostic {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(DiagnosticState::default())),
        }
    }

    pub(crate) fn invocation_root(&self) -> Option<DiagnosticInvocation> {
        self.inner.lock().root_snapshot()
    }
}

impl fmt::Debug for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Diagnostic").finish()
    }
}

impl IDiagnostic for Diagnostic {
    fn initialized(&mut self, _engine: &mut ApplicationEngine) {}

    fn disposed(&mut self) {}

    fn context_loaded(&mut self, context: &ExecutionContext) {
        let script_hash = {
            let state_arc = context
                .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
            let script_hash = state_arc.lock().script_hash;
            script_hash
        };

        if let Some(script_hash) = script_hash {
            let mut inner = self.inner.lock();
            let next_node = match inner.current_node {
                Some(parent) => inner.add_child(parent, script_hash),
                None => inner.add_root(script_hash),
            };
            inner.current_node = Some(next_node);
        }
    }

    fn context_unloaded(&mut self, _context: &ExecutionContext) {
        let mut inner = self.inner.lock();
        inner.current_node = inner
            .current_node
            .and_then(|current| inner.nodes.get(current).and_then(|node| node.parent));
    }

    fn pre_execute_instruction(&mut self, _instruction: &Instruction) {}

    fn post_execute_instruction(&mut self, _instruction: &Instruction) {}
}
