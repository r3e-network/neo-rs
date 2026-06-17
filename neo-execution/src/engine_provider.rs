//! Application engine provider trait.
//!
//! Corresponds to C# `Neo.SmartContract.IApplicationEngineProvider`. This trait
//! allows plugins and extensions to provide custom `ApplicationEngine` subclasses
//! for specialized execution environments.

use neo_storage::DataCache;
use neo_vm::JumpTable;

use crate::ApplicationEngine;
use crate::diagnostic::Diagnostic;
use neo_primitives::TriggerType;

/// A provider for creating [`ApplicationEngine`] instances.
///
/// Implement this trait to customize how application engines are created.
/// The default implementation is used when no custom provider is registered.
///
/// Corresponds to C# `Neo.SmartContract.IApplicationEngineProvider`.
pub trait ApplicationEngineProvider: Send + Sync {
    /// Creates a new [`ApplicationEngine`] instance.
    ///
    /// This method is called by `ApplicationEngine::create` to allow
    /// customization of the engine construction.
    ///
    /// # Arguments
    ///
    /// * `trigger` - The trigger type (Application or Verification)
    /// * `snapshot` - The data cache snapshot for state access
    /// * `gas` - The maximum gas allowed for this execution
    /// * `diagnostic` - Optional diagnostic callback interface
    /// * `jump_table` - The jump table for opcode dispatch
    fn create(
        &self,
        trigger: TriggerType,
        snapshot: &DataCache,
        gas: i64,
        diagnostic: Option<Box<dyn Diagnostic>>,
        jump_table: JumpTable,
    ) -> ApplicationEngine;
}
