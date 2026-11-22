//! Interop service registry aligned with Neo's C# implementation.
//!
//! The interop service maps syscall names to descriptors, computes their
//! hashes, and dispatches execution either to a VM-side callback or to the host
//! environment (e.g. `ApplicationEngine`).
//!
//! The VM owns this *machinery* only. Neo host-service metadata — canonical
//! names, GAS base prices and required [`CallFlags`] — is owned by the layer
//! that implements the service (`ApplicationEngine` in `neo-smart-contract`)
//! and reaches the VM exclusively through [`InteropService::register`].
//! There is no VM-side built-in syscall table, so the implementation that
//! enforces a permission and the metadata that declares it cannot disagree.

use crate::Instruction;
use crate::error::{VmError, VmResult};
use crate::execution_context::ExecutionContext;
use crate::execution_engine::ExecutionEngine;
use neo_primitives::CallFlags;
use std::collections::HashMap;
use std::str;
use std::sync::Arc;

/// Function pointer used for interop handlers that execute within the VM itself.
pub type InteropCallback = fn(&mut ExecutionEngine) -> VmResult<()>;

/// Borrow-free view of a dynamically registered syscall.
///
/// Dispatch copies this out before invoking the handler so a nested syscall
/// still sees the installed [`InteropService`].
#[derive(Clone, Copy)]
pub(crate) struct DynamicDispatch {
    pub handler: Option<InteropCallback>,
    pub required_call_flags: CallFlags,
    /// `true` when the descriptor carries no VM-side callback, so the installed
    /// [`InteropHost`] owns both the implementation and the enforcement of the
    /// registered price and call flags.
    pub host_owned: bool,
}

/// Descriptor for a syscall registered with the interop service. Mirrors the shape of
/// `Neo.SmartContract.InteropDescriptor` while keeping Rust ergonomics.
#[derive(Clone)]
pub struct VmInteropDescriptor {
    /// Canonical name of the syscall (e.g. `System.Runtime.Platform`).
    pub name: String,
    /// Optional handler executed directly by the VM. When `None`, the call is delegated
    /// to the configured [`InteropHost`].
    pub handler: Option<InteropCallback>,
    /// Fixed price charged by the syscall (in execution units).
    ///
    /// In the Neo N3 reference implementation, the host (`ApplicationEngine`) applies
    /// scaling (e.g. `ExecFeeFactor`) and enforces gas limits. The VM stores the value
    /// for lookup/introspection but does not charge it directly.
    pub price: i64,
    /// Required call flags to run the syscall.
    pub required_call_flags: CallFlags,
}

/// Internal representation of a registered descriptor including its computed hash.
#[derive(Clone)]
struct RegisteredDescriptor {
    descriptor: VmInteropDescriptor,
    hash: u32,
}

impl RegisteredDescriptor {
    fn new(descriptor: VmInteropDescriptor) -> VmResult<Self> {
        let hash = hash_syscall(&descriptor.name)?;
        Ok(Self { descriptor, hash })
    }
}

/// Host interface used for forwarding interop calls that require external context
/// (for example `ApplicationEngine`).
///
/// # Security note
/// The VM does not perform semantic authorization of syscalls beyond checking
/// registration, fixed gas price charging, and required call flags. As in the C#
/// implementation, syscall security (permissions, container checks, policy rules)
/// is enforced by the host (`ApplicationEngine` / native contract layer).
pub trait InteropHost {
    /// Invokes a system call identified by its hash.
    ///
    /// # Arguments
    /// * `engine` - The execution engine
    /// * `hash` - The syscall hash identifier
    fn invoke_syscall(&mut self, engine: &mut ExecutionEngine, hash: u32) -> VmResult<()>;

    /// Called when a new execution context is loaded onto the invocation stack.
    fn on_context_loaded(
        &mut self,
        _engine: &mut ExecutionEngine,
        _context: &ExecutionContext,
    ) -> VmResult<()> {
        Ok(())
    }

    /// Called when an execution context is unloaded from the invocation stack.
    fn on_context_unloaded(
        &mut self,
        _engine: &mut ExecutionEngine,
        _context: &ExecutionContext,
    ) -> VmResult<()> {
        Ok(())
    }

    /// Called before executing an instruction. Allows the host to intercept execution.
    ///
    /// If the host needs the current execution context it can obtain it via
    /// `engine.current_context()` — this avoids an expensive per-instruction
    /// `ExecutionContext` clone.
    fn pre_execute_instruction(
        &mut self,
        _engine: &mut ExecutionEngine,
        _instruction: &Instruction,
    ) -> VmResult<()> {
        Ok(())
    }

    /// Called after executing an instruction. Allows the host to perform post-processing.
    ///
    /// `instruction` is `None` for the implicit `RET` that the engine substitutes
    /// when the instruction pointer reaches the end of the script; the host
    /// charges no instruction price for it. `run_stats` holds the statistics
    /// collected by the handler and drives the post-`HF_Huyao` opcode pricing
    /// (neo-vm v3.11.2 `PostExecuteInstruction(instruction, runStats)`).
    ///
    /// If the host needs the current execution context it can obtain it via
    /// `engine.current_context()` — this avoids an expensive per-instruction
    /// `ExecutionContext` clone.
    fn post_execute_instruction(
        &mut self,
        _engine: &mut ExecutionEngine,
        _instruction: Option<&Instruction>,
        _run_stats: &crate::run_stats::RunStats,
    ) -> VmResult<()> {
        Ok(())
    }

    /// Called when CALLT opcode is executed. The host should resolve the method token
    /// and perform the cross-contract call.
    ///
    /// # Arguments
    /// * `engine` - The execution engine
    /// * `token_id` - The method token index from the instruction operand
    ///
    /// # Returns
    /// Default implementation returns an error indicating CALLT requires `ApplicationEngine`.
    fn on_callt(&mut self, _engine: &mut ExecutionEngine, token_id: u16) -> VmResult<()> {
        Err(VmError::invalid_operation_msg(format!(
            "CALLT (token {token_id}) requires ApplicationEngine context. \
             This opcode cannot be executed in standalone VM mode."
        )))
    }
}

/// `InteropService` manages syscall descriptors and dispatches them just like the C# implementation.
///
/// Cloning shares the immutable registry. Registration uses copy-on-write so a
/// host-specific descriptor can never mutate another engine's service.
#[derive(Clone, Default)]
pub struct InteropService {
    descriptors: Arc<HashMap<u32, RegisteredDescriptor>>,
}

impl InteropService {
    /// Creates a new, empty interop service.
    ///
    /// Descriptors must be registered explicitly by the host (mirroring the
    /// static registration that happens in C#). The VM deliberately ships no
    /// built-in Neo syscall table: names, prices and required call flags are
    /// owned by the registering layer (`ApplicationEngine` for Neo host
    /// services), so there is exactly one authority per engine.
    #[must_use]
    pub fn new() -> Self {
        Self {
            descriptors: Arc::new(HashMap::new()),
        }
    }

    /// Registers a descriptor and returns its syscall hash.
    pub fn register(&mut self, descriptor: VmInteropDescriptor) -> VmResult<u32> {
        let registered = RegisteredDescriptor::new(descriptor)?;

        if self.descriptors.contains_key(&registered.hash) {
            return Err(VmError::invalid_operation_msg(format!(
                "Syscall {} already registered",
                registered.descriptor.name
            )));
        }

        let hash = registered.hash;
        Arc::make_mut(&mut self.descriptors).insert(hash, registered);
        Ok(hash)
    }

    /// Registers a host-only descriptor (handled by the execution engine host) and returns its hash.
    pub fn register_host_descriptor(
        &mut self,
        name: &str,
        price: i64,
        required_call_flags: CallFlags,
    ) -> VmResult<u32> {
        self.register(VmInteropDescriptor {
            name: name.to_string(),
            handler: None,
            price,
            required_call_flags,
        })
    }

    /// Retrieves a descriptor by name (ASCII byte slice).
    #[must_use]
    pub fn get_method(&self, name: &[u8]) -> Option<&VmInteropDescriptor> {
        let name_str = str::from_utf8(name).ok()?;
        let hash = hash_syscall(name_str).ok()?;
        self.descriptors.get(&hash).map(|entry| &entry.descriptor)
    }

    /// Returns the fixed price for a syscall by name. Returns 0 if not found.
    #[must_use]
    pub fn get_price(&self, name: &[u8]) -> i64 {
        self.get_method(name).map_or(0, |d| d.price)
    }

    /// Invokes a syscall linked to an instruction.
    pub fn invoke_instruction(
        &mut self,
        engine: &mut ExecutionEngine,
        instruction: &Instruction,
    ) -> VmResult<()> {
        let hash = instruction.token_u32();
        self.invoke_by_hash(engine, hash)
    }

    /// Copies the dynamic registration for `hash`, if one exists.
    pub(crate) fn dynamic_dispatch(&self, hash: u32) -> Option<DynamicDispatch> {
        self.descriptors.get(&hash).map(|entry| DynamicDispatch {
            handler: entry.descriptor.handler,
            required_call_flags: entry.descriptor.required_call_flags,
            host_owned: entry.descriptor.handler.is_none(),
        })
    }

    /// Returns the canonical name registered for `hash`, if one exists.
    ///
    /// Diagnostics only: the dispatch hot path copies the `Copy` fields from
    /// [`DynamicDispatch`] and never formats a name.
    pub(crate) fn syscall_name(&self, hash: u32) -> Option<&str> {
        self.descriptors
            .get(&hash)
            .map(|entry| entry.descriptor.name.as_str())
    }

    /// Invokes a syscall by its 32-bit hash identifier.
    ///
    /// Dispatch is entirely registration driven. The VM owns no Neo syscall
    /// names, prices or permission bits; it only routes a hash to the
    /// descriptor the host registered for this engine.
    pub fn invoke_by_hash(&mut self, engine: &mut ExecutionEngine, hash: u32) -> VmResult<()> {
        let Some(prepared) = self.dynamic_dispatch(hash) else {
            return Err(VmError::invalid_operation_msg(format!(
                "Syscall 0x{hash:08x} not registered"
            )));
        };

        // Host-owned services are priced and permission-checked by the host
        // from the very descriptor registered above (`ApplicationEngine`
        // enforces both in `invoke_syscall`), so the VM must not keep a second
        // copy of that policy.
        if prepared.host_owned {
            return match engine.invoke_host_syscall(hash) {
                Some(result) => result,
                None => Err(VmError::invalid_operation_msg(format!(
                    "Syscall {} requires an interop host",
                    self.syscall_name(hash).unwrap_or("<unregistered>")
                ))),
            };
        }

        if !engine.has_call_flags(prepared.required_call_flags) {
            return Err(VmError::invalid_operation_msg(format!(
                "Missing required call flags: {:?}",
                prepared.required_call_flags
            )));
        }

        match prepared.handler {
            Some(callback) => callback(engine),
            None => Err(VmError::invalid_operation_msg(format!(
                "Syscall 0x{hash:08x} has no handler"
            ))),
        }
    }

    /// Returns the number of registered descriptors (useful for diagnostics/tests).
    #[must_use]
    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    /// Returns whether the service has no registered descriptors.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }
}

fn hash_syscall(api: &str) -> VmResult<u32> {
    if api.len() > 252 {
        return Err(VmError::invalid_operation_msg(format!(
            "Syscall API too long: {} bytes (max 252)",
            api.len()
        )));
    }

    Ok(crate::interop_hash(api))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_registration_does_not_mutate_cloned_service() {
        let mut original = InteropService::new();
        original
            .register_host_descriptor("Test.Shared", 3, CallFlags::READ_STATES)
            .expect("shared descriptor");
        let mut private = original.clone();

        private
            .register_host_descriptor("Test.Private", 7, CallFlags::WRITE_STATES)
            .expect("private descriptor");

        assert!(original.get_method(b"Test.Private").is_none());
        assert_eq!(private.get_price(b"Test.Private"), 7);
        assert_eq!(original.get_price(b"Test.Shared"), 3);
        assert_eq!(private.get_price(b"Test.Shared"), 3);
        assert_eq!(original.len(), 1);
        assert_eq!(private.len(), 2);
    }
}
