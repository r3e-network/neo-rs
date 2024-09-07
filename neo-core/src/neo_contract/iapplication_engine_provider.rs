use neo_network_p2p_payloads::TriggerType;
use neo_persistence::DataCache;
use neo_vm::{ApplicationEngine, IDiagnostic, JumpTable};
use crate::{Block, IVerifiable, ProtocolSettings};

/// A provider for creating `ApplicationEngine` instances.
pub trait IApplicationEngineProvider {
    /// Creates a new instance of the `ApplicationEngine` struct or its subtype.
    /// This method will be called by `ApplicationEngine::create()`.
    ///
    /// # Arguments
    ///
    /// * `trigger` - The trigger of the execution.
    /// * `container` - The container of the script.
    /// * `snapshot` - The snapshot used by the engine during execution.
    /// * `persisting_block` - The block being persisted. It should be `None` if the `trigger` is `TriggerType::Verification`.
    /// * `settings` - The `ProtocolSettings` used by the engine.
    /// * `gas` - The maximum gas used in this execution. The execution will fail when the gas is exhausted.
    /// * `diagnostic` - The diagnostic to be used by the `ApplicationEngine`.
    /// * `jump_table` - The jump table to be used by the `ApplicationEngine`.
    ///
    /// # Returns
    ///
    /// The engine instance created.
    fn create(
        &self,
        trigger: TriggerType,
        container: &dyn IVerifiable,
        snapshot: DataCache,
        persisting_block: Option<Block>,
        settings: ProtocolSettings,
        gas: i64,
        diagnostic: Box<dyn IDiagnostic>,
        jump_table: JumpTable,
    ) -> ApplicationEngine;
}
