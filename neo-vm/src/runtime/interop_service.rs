//! Interop service registry aligned with Neo's C# implementation.
//!
//! The interop service is responsible for mapping syscall names to descriptors,
//! computing their hashes, and dispatching execution either to built-in handlers
//! or to the host environment (e.g. `ApplicationEngine`).

use crate::error::{VmError, VmResult};
use crate::execution_context::ExecutionContext;
use crate::execution_engine::ExecutionEngine;
use neo_primitives::CallFlags;
use neo_vm_rs::Instruction;
use rustc_hash::FxHashMap;
use std::borrow::Cow;
use std::collections::hash_map::Entry;
use std::str;

/// Function pointer used for interop handlers that execute within the VM itself.
pub type InteropCallback<S = ()> = fn(&mut ExecutionEngine<S>) -> VmResult<()>;

/// Descriptor for a syscall registered with the interop service. Mirrors the shape of
/// `Neo.SmartContract.InteropDescriptor` while keeping Rust ergonomics.
#[derive(Clone)]
pub struct VmInteropDescriptor<S = ()> {
    /// Canonical name of the syscall (e.g. `System.Runtime.Platform`).
    ///
    /// Protocol descriptors use borrowed static names, avoiding an allocation
    /// each time an application engine builds its syscall registry. Custom VM
    /// hosts may still supply an owned name.
    pub name: Cow<'static, str>,
    /// Optional handler executed directly by the VM. When `None`, the call is delegated
    /// to the configured [`InteropHost`].
    pub handler: Option<InteropCallback<S>>,
    /// Fixed price charged by the syscall (in execution units).
    ///
    /// In the Neo N3 reference implementation, the host (`ApplicationEngine`) applies
    /// scaling (e.g. `ExecFeeFactor`) and enforces gas limits. The VM stores the value
    /// for lookup/introspection but does not charge it directly.
    pub price: i64,
    /// Required call flags to run the syscall.
    pub required_call_flags: CallFlags,
}

/// Internal descriptor representation stored under its precomputed hash key.
struct RegisteredDescriptor<S = ()> {
    descriptor: VmInteropDescriptor<S>,
}

// Syscall IDs are closed, protocol-derived u32 values rather than attacker-
// chosen keys. FxHashMap avoids SipHash overhead without changing lookup or
// duplicate-detection semantics; callers must not reuse this choice for
// untrusted key material or consensus-visible iteration.
type DescriptorMap<S> = FxHashMap<u32, RegisteredDescriptor<S>>;

impl<S> RegisteredDescriptor<S> {
    fn new(descriptor: VmInteropDescriptor<S>) -> VmResult<(u32, Self)> {
        let hash = hash_syscall(descriptor.name.as_ref())?;
        Ok((hash, Self { descriptor }))
    }
}

pub(crate) struct ResolvedInterop<S = ()> {
    pub(crate) handler: Option<InteropCallback<S>>,
    pub(crate) required_call_flags: CallFlags,
}

impl<S> Copy for ResolvedInterop<S> {}

impl<S> Clone for ResolvedInterop<S> {
    fn clone(&self) -> Self {
        *self
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
pub trait InteropHost<S = ()> {
    /// Invokes a system call identified by its hash.
    ///
    /// # Arguments
    /// * `engine` - The execution engine
    /// * `hash` - The syscall hash identifier
    fn invoke_syscall(&mut self, engine: &mut ExecutionEngine<S>, hash: u32) -> VmResult<()>;

    /// Called when a new execution context is loaded onto the invocation stack.
    fn on_context_loaded(
        &mut self,
        _engine: &mut ExecutionEngine<S>,
        _context: &ExecutionContext<S>,
    ) -> VmResult<()> {
        Ok(())
    }
    /// Called when an execution context is unloaded from the invocation stack.
    fn on_context_unloaded(
        &mut self,
        _engine: &mut ExecutionEngine<S>,
        _context: &ExecutionContext<S>,
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
        _engine: &mut ExecutionEngine<S>,
        _instruction: &Instruction,
    ) -> VmResult<()> {
        Ok(())
    }

    /// Called after executing an instruction. Allows the host to perform post-processing.
    ///
    /// If the host needs the current execution context it can obtain it via
    /// `engine.current_context()` — this avoids an expensive per-instruction
    /// `ExecutionContext` clone.
    fn post_execute_instruction(
        &mut self,
        _engine: &mut ExecutionEngine<S>,
        _instruction: &Instruction,
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
    fn on_callt(&mut self, _engine: &mut ExecutionEngine<S>, token_id: u16) -> VmResult<()> {
        Err(VmError::invalid_operation_msg(format!(
            "CALLT (token {token_id}) requires ApplicationEngine context. \
             This opcode cannot be executed in standalone VM mode."
        )))
    }
}

/// `InteropService` manages syscall descriptors and dispatches them just like the C# implementation.
pub struct InteropService<S = ()> {
    descriptors: DescriptorMap<S>,
}

impl<S> Default for InteropService<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> InteropService<S> {
    /// Creates a new, empty interop service. Descriptors must be registered explicitly
    /// by the host (mirroring the static registration that happens in C#).
    #[must_use]
    pub fn new() -> Self {
        Self {
            descriptors: FxHashMap::default(),
        }
    }

    /// Creates an empty interop service with storage for at least `capacity`
    /// descriptors.
    ///
    /// Hosts with a fixed protocol catalog should use this constructor to avoid
    /// repeated table growth while registering their descriptors.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            descriptors: FxHashMap::with_capacity_and_hasher(capacity, Default::default()),
        }
    }

    /// Registers a descriptor and returns its syscall hash.
    pub fn register(&mut self, descriptor: VmInteropDescriptor<S>) -> VmResult<u32> {
        let (hash, registered) = RegisteredDescriptor::new(descriptor)?;

        match self.descriptors.entry(hash) {
            Entry::Vacant(entry) => {
                entry.insert(registered);
                Ok(hash)
            }
            Entry::Occupied(_) => Err(VmError::invalid_operation_msg(format!(
                "Syscall {} already registered",
                registered.descriptor.name
            ))),
        }
    }

    /// Registers a host-only descriptor (handled by the execution engine host) and returns its hash.
    pub fn register_host_descriptor(
        &mut self,
        name: &'static str,
        price: i64,
        required_call_flags: CallFlags,
    ) -> VmResult<u32> {
        self.register(VmInteropDescriptor {
            name: Cow::Borrowed(name),
            handler: None,
            price,
            required_call_flags,
        })
    }

    /// Retrieves a descriptor by name (ASCII byte slice).
    #[must_use]
    pub fn get_method(&self, name: &[u8]) -> Option<&VmInteropDescriptor<S>> {
        let name_str = str::from_utf8(name).ok()?;
        let hash = hash_syscall(name_str).ok()?;
        self.descriptors.get(&hash).map(|entry| &entry.descriptor)
    }

    /// Returns the fixed price for a syscall by name. Returns 0 if not found.
    #[must_use]
    pub fn get_price(&self, name: &[u8]) -> i64 {
        self.get_method(name).map_or(0, |d| d.price)
    }

    /// Iterates over registered descriptor metadata in unspecified order.
    pub fn registered_descriptors(&self) -> impl Iterator<Item = (&str, i64, CallFlags)> + '_ {
        self.descriptors.values().map(|entry| {
            (
                entry.descriptor.name.as_ref(),
                entry.descriptor.price,
                entry.descriptor.required_call_flags,
            )
        })
    }

    /// Invokes a syscall linked to an instruction.
    pub fn invoke_instruction(
        &self,
        engine: &mut ExecutionEngine<S>,
        instruction: &Instruction,
    ) -> VmResult<()> {
        let hash = instruction.token_u32();
        self.invoke_by_hash(engine, hash)
    }

    /// Invokes a syscall by its 32-bit hash identifier.
    pub fn invoke_by_hash(&self, engine: &mut ExecutionEngine<S>, hash: u32) -> VmResult<()> {
        let resolved = self.resolve_by_hash(hash)?;

        if !engine.has_call_flags(resolved.required_call_flags) {
            return Err(VmError::invalid_operation_msg(format!(
                "Missing required call flags: {:?}",
                resolved.required_call_flags
            )));
        }

        if let Some(callback) = resolved.handler {
            return callback(engine);
        }

        if let Some(result) = engine.invoke_host_syscall(hash) {
            result
        } else {
            Err(VmError::invalid_operation_msg(format!(
                "Syscall {} requires an interop host",
                self.descriptor_name(hash).unwrap_or("<unknown>")
            )))
        }
    }

    #[inline]
    pub(crate) fn resolve_by_hash(&self, hash: u32) -> VmResult<ResolvedInterop<S>> {
        let entry = self.descriptors.get(&hash).ok_or_else(|| {
            VmError::invalid_operation_msg(format!("Syscall 0x{hash:08x} not registered"))
        })?;
        Ok(ResolvedInterop {
            handler: entry.descriptor.handler,
            required_call_flags: entry.descriptor.required_call_flags,
        })
    }

    #[inline]
    pub(crate) fn descriptor_name(&self, hash: u32) -> Option<&str> {
        self.descriptors
            .get(&hash)
            .map(|entry| entry.descriptor.name.as_ref())
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

    Ok(neo_vm_rs::interop_hash(api))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_registry_visible(engine: &mut ExecutionEngine) -> VmResult<()> {
        if engine.interop_service().is_some() {
            Ok(())
        } else {
            Err(VmError::invalid_operation_msg(
                "interop registry was removed during syscall dispatch",
            ))
        }
    }

    #[test]
    fn syscall_callbacks_keep_the_registry_installed() {
        let mut engine = ExecutionEngine::new(None);
        let hash = engine
            .interop_service_mut()
            .expect("default interop service")
            .register(VmInteropDescriptor {
                name: Cow::Borrowed("System.Test.RegistryVisible"),
                handler: Some(assert_registry_visible),
                price: 0,
                required_call_flags: CallFlags::NONE,
            })
            .expect("register test syscall");

        engine.on_syscall(hash).expect("dispatch test syscall");
        assert_eq!(engine.interop_service().map(InteropService::len), Some(1));
    }
}
