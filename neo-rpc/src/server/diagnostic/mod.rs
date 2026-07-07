//! # neo-rpc::server::diagnostic
//!
//! RPC diagnostic endpoints and health reporting helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `invocation_tree`: invocation-tree capture state and snapshots.

mod invocation_tree;

use neo_execution::ApplicationEngine;
use neo_execution::diagnostic::Diagnostic as DiagnosticTrait;
use neo_execution::execution_context_state::ExecutionContextState;
use neo_vm::execution_context::ExecutionContext;
use neo_vm_rs::Instruction;
use parking_lot::Mutex;
use std::fmt;
use std::sync::Arc;

pub(crate) use invocation_tree::DiagnosticInvocation;
use invocation_tree::InvocationTree;

/// Diagnostics helper mirroring `Neo.Plugins.RpcServer.Diagnostic`.
#[derive(Clone, Default)]
pub struct Diagnostic {
    inner: Arc<Mutex<InvocationTree>>,
}

impl Diagnostic {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(InvocationTree::default())),
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

impl DiagnosticTrait for Diagnostic {
    fn initialized(&mut self, _engine: &mut ApplicationEngine) {}

    fn disposed(&mut self) {}

    fn context_loaded(&mut self, context: &ExecutionContext) {
        let script_hash = {
            let state_arc = context
                .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);

            state_arc.lock().script_hash
        };

        if let Some(script_hash) = script_hash {
            self.inner.lock().load_context(script_hash);
        }
    }

    fn context_unloaded(&mut self, _context: &ExecutionContext) {
        self.inner.lock().unload_context();
    }

    fn pre_execute_instruction(&mut self, _instruction: &Instruction) {}

    fn post_execute_instruction(&mut self, _instruction: &Instruction) {}
}
