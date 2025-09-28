//! IApplicationEngineProvider - matches C# Neo.SmartContract.IApplicationEngineProvider exactly

use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::i_diagnostic::IDiagnostic;
use crate::smart_contract::trigger_type::TriggerType;
use crate::IVerifiable;

/// A provider for creating ApplicationEngine instances (matches C# IApplicationEngineProvider)
pub trait IApplicationEngineProvider {
    /// Creates a new instance of the ApplicationEngine class or its subclass
    fn create(
        &self,
        trigger: TriggerType,
        container: Box<dyn IVerifiable>,
        snapshot: DataCache,
        persisting_block: Option<Block>,
        settings: ProtocolSettings,
        gas: i64,
        diagnostic: Option<Box<dyn IDiagnostic>>,
        jump_table: Option<JumpTable>,
    ) -> ApplicationEngine;
}

/// Placeholder for DataCache from persistence
#[derive(Clone, Debug)]
pub struct DataCache {
    pub id: u32,
}

/// Placeholder for Block from ledger
#[derive(Clone, Debug)]
pub struct Block {
    pub index: u32,
    pub timestamp: u64,
}

/// Placeholder for ProtocolSettings
#[derive(Clone, Debug)]
pub struct ProtocolSettings {
    pub magic: u32,
    pub address_version: u8,
}

/// Placeholder for JumpTable from VM
#[derive(Clone, Debug)]
pub struct JumpTable {
    pub handlers: std::collections::HashMap<u8, fn(&mut ApplicationEngine) -> Result<(), String>>,
}
