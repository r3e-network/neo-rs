//! Interop service registry aligned with Neo's C# implementation.
//!
//! The interop service is responsible for mapping syscall names to descriptors,
//! computing their hashes, and dispatching execution either to built-in handlers
//! or to the host environment (e.g. `ApplicationEngine`).

use crate::call_flags::CallFlags;
use crate::error::{VmError, VmResult};
use crate::execution_context::ExecutionContext;
use crate::execution_engine::ExecutionEngine;
use crate::instruction::Instruction;
use crate::script_builder::ScriptBuilder;
use std::collections::HashMap;
use std::str;

/// Function pointer used for interop handlers that execute within the VM itself.
pub type InteropCallback = fn(&mut ExecutionEngine) -> VmResult<()>;

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
struct RegisteredDescriptor {
    descriptor: VmInteropDescriptor,
    hash: u32,
}

impl RegisteredDescriptor {
    fn new(descriptor: VmInteropDescriptor) -> VmResult<Self> {
        let hash = ScriptBuilder::hash_syscall(&descriptor.name)?;
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
    fn invoke_syscall(&mut self, engine: &mut ExecutionEngine, hash: u32) -> VmResult<()>;

    fn on_context_loaded(
        &mut self,
        _engine: &mut ExecutionEngine,
        _context: &ExecutionContext,
    ) -> VmResult<()> {
        Ok(())
    }
    fn on_context_unloaded(
        &mut self,
        _engine: &mut ExecutionEngine,
        _context: &ExecutionContext,
    ) -> VmResult<()> {
        Ok(())
    }

    fn pre_execute_instruction(
        &mut self,
        _engine: &mut ExecutionEngine,
        _context: &ExecutionContext,
        _instruction: &Instruction,
    ) -> VmResult<()> {
        Ok(())
    }

    fn post_execute_instruction(
        &mut self,
        _engine: &mut ExecutionEngine,
        _context: &ExecutionContext,
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
    fn on_callt(&mut self, _engine: &mut ExecutionEngine, token_id: u16) -> VmResult<()> {
        Err(VmError::invalid_operation_msg(format!(
            "CALLT (token {token_id}) requires ApplicationEngine context. \
             This opcode cannot be executed in standalone VM mode."
        )))
    }
}

/// `InteropService` manages syscall descriptors and dispatches them just like the C# implementation.
#[derive(Default)]
pub struct InteropService {
    descriptors: HashMap<u32, RegisteredDescriptor>,
}

impl InteropService {
    /// Creates a new, empty interop service. Descriptors must be registered explicitly
    /// by the host (mirroring the static registration that happens in C#).
    #[must_use] 
    pub fn new() -> Self {
        Self {
            descriptors: HashMap::new(),
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
        self.descriptors.insert(hash, registered);
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
        let hash = ScriptBuilder::hash_syscall(name_str).ok()?;
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

    /// Invokes a syscall by its 32-bit hash identifier.
    pub fn invoke_by_hash(&mut self, engine: &mut ExecutionEngine, hash: u32) -> VmResult<()> {
        let (handler, required_call_flags, name) = {
            let entry = self.descriptors.get(&hash).ok_or_else(|| {
                VmError::invalid_operation_msg(format!("Syscall 0x{hash:08x} not registered"))
            })?;

            (
                entry.descriptor.handler,
                entry.descriptor.required_call_flags,
                entry.descriptor.name.clone(),
            )
        };

        if !engine.has_call_flags(required_call_flags) {
            return Err(VmError::invalid_operation_msg(format!(
                "Missing required call flags: {required_call_flags:?}"
            )));
        }

        if let Some(callback) = handler {
            return callback(engine);
        }

        if let Some(host_ptr) = engine.interop_host_ptr() {
            // Safety: the pointer originates from the engine's stored host reference
            // and remains valid for the duration of this call.
            unsafe { (*host_ptr).invoke_syscall(engine, hash) }
        } else {
            Err(VmError::invalid_operation_msg(format!(
                "Syscall {name} requires an interop host"
            )))
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
