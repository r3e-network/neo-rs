use NeoRust::builder::ScriptBuilder;
use NeoRust::prelude::VMState;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::call_flags::CallFlags;
use crate::protocol_settings::ProtocolSettings;
use crate::store::Snapshot;
use crate::uint160::UInt160;

/// Represents the descriptor of an asset.
pub struct AssetDescriptor {
    /// The id of the asset.
    pub asset_id: UInt160,

    /// The name of the asset.
    pub asset_name: String,

    /// The symbol of the asset.
    pub symbol: String,

    /// The number of decimal places of the token.
    pub decimals: u8,
}

impl AssetDescriptor {
    /// Initializes a new instance of the AssetDescriptor struct.
    ///
    /// # Arguments
    ///
    /// * `snapshot` - The snapshot used to read data.
    /// * `settings` - The ProtocolSettings used by the ApplicationEngine.
    /// * `asset_id` - The id of the asset.
    ///
    /// # Returns
    ///
    /// A Result containing the new AssetDescriptor instance or an error.
    pub fn new(snapshot: &Snapshot, settings: &ProtocolSettings, asset_id: UInt160) -> Result<Self, String> {
        let contract = ContractManagement::get_contract(snapshot, &asset_id)
            .ok_or_else(|| "Invalid asset_id".to_string())?;

        let script = ScriptBuilder::new()
            .contract_call(&asset_id, "decimals", CallFlags::READ_ONLY, CallFlags::all())
            .unwrap()
            .contract_call(&asset_id, "symbol", CallFlags::READ_ONLY, CallFlags::all())

            .to_array();

        let engine = ApplicationEngine::run(&script, snapshot, settings, 30_000_000)?;
        if engine.state() != VMState::Halt {
            return Err("Execution failed".to_string());
        }

        let mut stack = engine.result_stack();
        let symbol = stack.pop().get_string()?;
        let decimals = stack.pop().get_integer()? as u8;

        Ok(Self {
            asset_id,
            asset_name: contract.manifest.name,
            symbol,
            decimals,
        })
    }
}

impl ToString for AssetDescriptor {
    fn to_string(&self) -> String {
        self.asset_name.clone()
    }
}
