//! High level application engine emulating Neo's `ApplicationEngine` behaviour.
//!
//! This module wraps [`ExecutionEngine`] providing gas tracking, syscall dispatch,
//! and notification handling consistent with the C# implementation. The primary
//! goal is parity with the unit tests from `Neo.VM.Tests`.

use crate::call_flags::CallFlags;
use crate::error::{VmError, VmResult};
use crate::execution_engine::ExecutionEngine;
use crate::execution_engine::VMState;
use crate::interop_service::{InteropHost, InteropService};
use crate::script::Script;
use crate::stack_item::StackItem;
use num_bigint::BigInt;
use std::collections::HashMap;

/// Trigger types mirrored from `Neo.SmartContract.TriggerType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TriggerType {
    OnPersist = 0x01,
    PostPersist = 0x02,
    Verification = 0x20,
    Application = 0x40,
    System = 0x03,
    All = 0x63,
}

/// Notification event emitted by smart contracts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationEvent {
    pub script_hash: Vec<u8>,
    pub name: String,
    pub arguments: Vec<StackItem>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ApplicationSyscall {
    RuntimePlatform,
    RuntimeGetTrigger,
    RuntimeGetTime,
    RuntimeLog,
    StorageGetContext,
    StoragePut,
    StorageGet,
}

/// High level application engine wrapper used for standalone VM testing.
pub struct VmApplicationEngine {
    engine: ExecutionEngine,
    trigger: TriggerType,
    gas_limit: u64,
    gas_consumed: u64,
    notifications: Vec<NotificationEvent>,
    snapshots: HashMap<Vec<u8>, Vec<u8>>,
    storage: HashMap<Vec<u8>, Vec<u8>>,
    syscall_map: HashMap<u32, ApplicationSyscall>,
}

impl VmApplicationEngine {
    /// Creates a new application engine for the specified trigger and gas limit.
    #[must_use] 
    pub fn new(trigger: TriggerType, gas_limit: u64) -> Self {
        let engine = ExecutionEngine::new(None);
        let mut app = Self {
            engine,
            trigger,
            gas_limit,
            gas_consumed: 0,
            notifications: Vec::new(),
            snapshots: HashMap::new(),
            storage: HashMap::new(),
            syscall_map: HashMap::new(),
        };

        app.register_default_syscalls()
            .expect("default syscalls must register");

        app
    }

    fn attach_host(&mut self) {
        let host_ptr: *mut Self = self;
        self.engine
            .set_interop_host(host_ptr as *mut dyn InteropHost);
    }

    fn interop_service_mut(&mut self) -> &mut InteropService {
        self.engine
            .interop_service_mut()
            .expect("ExecutionEngine always provides an interop service")
    }

    fn register_default_syscalls(&mut self) -> VmResult<()> {
        self.register_host_syscall(
            "System.Runtime.Platform",
            1,
            CallFlags::ALL,
            ApplicationSyscall::RuntimePlatform,
        )?;
        self.register_host_syscall(
            "System.Runtime.GetTrigger",
            1,
            CallFlags::ALL,
            ApplicationSyscall::RuntimeGetTrigger,
        )?;
        self.register_host_syscall(
            "System.Runtime.GetTime",
            1,
            CallFlags::ALL,
            ApplicationSyscall::RuntimeGetTime,
        )?;
        self.register_host_syscall(
            "System.Runtime.Log",
            1,
            CallFlags::ALL,
            ApplicationSyscall::RuntimeLog,
        )?;
        self.register_host_syscall(
            "System.Storage.GetContext",
            1,
            CallFlags::ALL,
            ApplicationSyscall::StorageGetContext,
        )?;
        self.register_host_syscall(
            "System.Storage.Put",
            1,
            CallFlags::ALL,
            ApplicationSyscall::StoragePut,
        )?;
        self.register_host_syscall(
            "System.Storage.Get",
            1,
            CallFlags::ALL,
            ApplicationSyscall::StorageGet,
        )?;
        Ok(())
    }

    fn register_host_syscall(
        &mut self,
        name: &str,
        price: i64,
        flags: CallFlags,
        syscall: ApplicationSyscall,
    ) -> VmResult<()> {
        let hash = self
            .interop_service_mut()
            .register_host_descriptor(name, price, flags)?;
        #[cfg(debug_assertions)]
        println!("Registered syscall {name} with hash 0x{hash:08x}");
        self.syscall_map.insert(hash, syscall);
        Ok(())
    }

    /// Executes the supplied script and returns the resulting VM state.
    pub fn execute(&mut self, script: Script) -> VMState {
        self.attach_host();

        let script_cost = script.len() as u64;
        if self.engine.load_script(script, -1, 0).is_err() {
            let _ = self.consume_gas(script_cost);
            self.engine.set_state(VMState::FAULT);
            return VMState::FAULT;
        }

        if let Some(context) = self.engine.current_context_mut() {
            context.set_rvcount(-1);
        }

        let state = self.engine.execute();
        let _ = self.consume_gas(script_cost);
        #[cfg(debug_assertions)]
        println!(
            "ApplicationEngine post-exec state: {:?}, result len: {}",
            state,
            self.engine.result_stack().len()
        );
        #[cfg(debug_assertions)]
        if let Some(err) = self.engine.uncaught_exception() {
            println!("Uncaught exception: {err:?}");
        }
        state
    }

    /// Consumes gas from the available budget.
    pub fn consume_gas(&mut self, amount: u64) -> VmResult<()> {
        let new_total = self.gas_consumed.saturating_add(amount);
        if new_total > self.gas_limit {
            return Err(VmError::invalid_operation_msg("Gas limit exceeded"));
        }
        self.gas_consumed = new_total;
        Ok(())
    }

    /// Returns total gas consumed.
    #[must_use] 
    pub const fn gas_consumed(&self) -> u64 {
        self.gas_consumed
    }

    /// Returns configured gas limit.
    #[must_use] 
    pub const fn gas_limit(&self) -> u64 {
        self.gas_limit
    }

    /// Returns current trigger.
    #[must_use] 
    pub const fn trigger(&self) -> TriggerType {
        self.trigger
    }

    /// Adds a notification event.
    pub fn add_notification(&mut self, notification: NotificationEvent) {
        self.notifications.push(notification);
    }

    /// Returns collected notifications.
    #[must_use] 
    pub fn notifications(&self) -> &[NotificationEvent] {
        &self.notifications
    }

    /// Stores an in-memory snapshot value.
    pub fn set_snapshot(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.snapshots.insert(key, value);
    }

    /// Retrieves a snapshot by key.
    #[must_use] 
    pub fn get_snapshot(&self, key: &[u8]) -> Option<&[u8]> {
        self.snapshots.get(key).map(std::vec::Vec::as_slice)
    }

    /// Returns a reference to the execution engine's result stack.
    #[must_use] 
    pub fn result_stack(&self) -> &crate::evaluation_stack::EvaluationStack {
        self.engine.result_stack()
    }

    /// Returns the current execution context.
    #[must_use] 
    pub fn current_context(&self) -> Option<&crate::execution_context::ExecutionContext> {
        self.engine.current_context()
    }

    /// Returns the effective call flags for this engine.
    #[must_use] 
    pub fn call_flags(&self) -> CallFlags {
        self.engine.call_flags()
    }
}

impl InteropHost for VmApplicationEngine {
    fn invoke_syscall(&mut self, engine: &mut ExecutionEngine, hash: u32) -> VmResult<()> {
        let syscall = self.syscall_map.get(&hash).copied().ok_or_else(|| {
            #[cfg(debug_assertions)]
            println!("Unknown syscall hash: 0x{hash:08x}");
            VmError::invalid_operation_msg("Unknown syscall")
        })?;

        match syscall {
            ApplicationSyscall::RuntimePlatform => {
                #[cfg(debug_assertions)]
                println!("Invoking System.Runtime.Platform");
                let context = engine
                    .current_context_mut()
                    .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
                context.push(StackItem::from_byte_string(b"NEO".to_vec()))?;
                let _ = self.consume_gas(1);
                Ok(())
            }
            ApplicationSyscall::RuntimeGetTrigger => {
                #[cfg(debug_assertions)]
                println!("Invoking System.Runtime.GetTrigger");
                let context = engine
                    .current_context_mut()
                    .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
                context.push(StackItem::from_int(BigInt::from(self.trigger as u8)))?;
                let _ = self.consume_gas(1);
                Ok(())
            }
            ApplicationSyscall::RuntimeGetTime => {
                #[cfg(debug_assertions)]
                println!("Invoking System.Runtime.GetTime");
                let context = engine
                    .current_context_mut()
                    .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
                context.push(StackItem::from_int(BigInt::from(0)))?;
                let _ = self.consume_gas(1);
                Ok(())
            }
            ApplicationSyscall::RuntimeLog => {
                #[cfg(debug_assertions)]
                println!("Invoking System.Runtime.Log");
                let context = engine
                    .current_context_mut()
                    .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
                let _ = context.evaluation_stack_mut().pop();
                let _ = self.consume_gas(1);
                Ok(())
            }
            ApplicationSyscall::StorageGetContext => {
                #[cfg(debug_assertions)]
                println!("Invoking System.Storage.GetContext");
                let context = engine
                    .current_context_mut()
                    .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
                context.push(StackItem::from_byte_string(vec![0u8; 20]))?;
                let _ = self.consume_gas(1);
                Ok(())
            }
            ApplicationSyscall::StoragePut => {
                #[cfg(debug_assertions)]
                println!("Invoking System.Storage.Put");
                let context = engine
                    .current_context_mut()
                    .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
                let value = context.evaluation_stack_mut().pop()?;
                let key = context.evaluation_stack_mut().pop()?;
                let _context = context.evaluation_stack_mut().pop()?;

                let key_bytes = key.as_bytes()?.clone();
                let value_bytes = value.as_bytes()?.clone();
                self.storage.insert(key_bytes, value_bytes);
                let _ = self.consume_gas(1);
                Ok(())
            }
            ApplicationSyscall::StorageGet => {
                #[cfg(debug_assertions)]
                println!("Invoking System.Storage.Get");
                let context = engine
                    .current_context_mut()
                    .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
                let key = context.evaluation_stack_mut().pop()?;
                let _context = context.evaluation_stack_mut().pop()?;
                let key_bytes = key.as_bytes()?.clone();

                if let Some(value) = self.storage.get(&key_bytes) {
                    context.push(StackItem::from_byte_string(value.clone()))?;
                } else {
                    context.push(StackItem::Null)?;
                }
                let _ = self.consume_gas(1);
                Ok(())
            }
        }
    }

    fn on_context_loaded(
        &mut self,
        _engine: &mut ExecutionEngine,
        _context: &crate::execution_context::ExecutionContext,
    ) -> VmResult<()> {
        Ok(())
    }
}

/// Backwards-compatible alias retaining the historical name used by the VM crate.
pub type ApplicationEngine = VmApplicationEngine;
