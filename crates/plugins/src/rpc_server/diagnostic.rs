use crate::rpc_server::{tree::Tree, tree_node::TreeNode};
use neo_core::smart_contract::execution_context_state::ExecutionContextState;
use neo_core::smart_contract::i_diagnostic::IDiagnostic;
use neo_core::smart_contract::ApplicationEngine;
use neo_core::UInt160;
use neo_vm::execution_context::ExecutionContext;
use neo_vm::instruction::Instruction;
use std::fmt;
use std::sync::{Arc, Mutex};

/// Diagnostics helper mirroring `Neo.Plugins.RpcServer.Diagnostic`.
#[derive(Clone, Default)]
pub struct Diagnostic {
    inner: Arc<Mutex<DiagnosticState>>,
}

#[derive(Default)]
struct DiagnosticState {
    invocation_tree: Tree<UInt160>,
    current_node: Option<Arc<Mutex<TreeNode<UInt160>>>>,
}

impl Diagnostic {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(DiagnosticState::default())),
        }
    }

    pub fn root(&self) -> Option<Arc<Mutex<TreeNode<UInt160>>>> {
        self.inner
            .lock()
            .ok()
            .and_then(|state| state.invocation_tree.root())
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
            state_arc.lock().ok().and_then(|state| state.script_hash)
        };

        if let Some(script_hash) = script_hash {
            if let Ok(mut inner) = self.inner.lock() {
                let next_node = match &inner.current_node {
                    Some(parent) => TreeNode::add_child(parent, script_hash),
                    None => inner.invocation_tree.add_root(script_hash),
                };
                inner.current_node = Some(next_node);
            }
        }
    }

    fn context_unloaded(&mut self, _context: &ExecutionContext) {
        if let Ok(mut inner) = self.inner.lock() {
            let parent_node = inner.current_node.as_ref().and_then(|current| {
                current
                    .lock()
                    .ok()
                    .and_then(|node| node.parent())
                    .and_then(|weak| weak.upgrade())
            });
            inner.current_node = parent_node;
        }
    }

    fn pre_execute_instruction(&mut self, _instruction: &Instruction) {}

    fn post_execute_instruction(&mut self, _instruction: &Instruction) {}
}
