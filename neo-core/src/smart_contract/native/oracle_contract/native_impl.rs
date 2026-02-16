use super::*;
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractParameterDefinition};
use crate::smart_contract::native::NativeContract;

impl NativeContract for OracleContract {
    fn supported_standards(
        &self,
        settings: &crate::protocol_settings::ProtocolSettings,
        block_height: u32,
    ) -> Vec<String> {
        if settings.is_hardfork_enabled(crate::hardfork::Hardfork::HfFaun, block_height) {
            vec!["NEP-30".to_string()]
        } else {
            Vec::new()
        }
    }

    fn activations(&self) -> Vec<crate::hardfork::Hardfork> {
        vec![crate::hardfork::Hardfork::HfFaun]
    }

    fn initialize(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let snapshot_arc = engine.snapshot_cache();
        let snapshot = snapshot_arc.as_ref();
        let key = self.price_key();
        if snapshot.try_get(&key).is_none() {
            self.put_item(
                snapshot,
                key,
                StorageItem::from_bytes(DEFAULT_PRICE.to_le_bytes().to_vec()),
            );
        }
        Ok(())
    }

    fn id(&self) -> i32 {
        self.id
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "OracleContract"
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn events(
        &self,
        _settings: &crate::protocol_settings::ProtocolSettings,
        _block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        vec![
            ContractEventDescriptor::new(
                "OracleRequest".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "Id".to_string(),
                        ContractParameterType::Integer,
                    )
                    .expect("OracleRequest.Id"),
                    ContractParameterDefinition::new(
                        "RequestContract".to_string(),
                        ContractParameterType::Hash160,
                    )
                    .expect("OracleRequest.RequestContract"),
                    ContractParameterDefinition::new(
                        "Url".to_string(),
                        ContractParameterType::String,
                    )
                    .expect("OracleRequest.Url"),
                    ContractParameterDefinition::new(
                        "Filter".to_string(),
                        ContractParameterType::String,
                    )
                    .expect("OracleRequest.Filter"),
                ],
            )
            .expect("OracleRequest event descriptor"),
            ContractEventDescriptor::new(
                "OracleResponse".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "Id".to_string(),
                        ContractParameterType::Integer,
                    )
                    .expect("OracleResponse.Id"),
                    ContractParameterDefinition::new(
                        "OriginalTx".to_string(),
                        ContractParameterType::Hash256,
                    )
                    .expect("OracleResponse.OriginalTx"),
                ],
            )
            .expect("OracleResponse event descriptor"),
        ]
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        self.invoke_method(engine, method, args)
    }

    fn post_persist(&self, engine: &mut ApplicationEngine) -> Result<()> {
        self.cleanup_persisted_responses(engine)?;
        self.reward_oracle_nodes(engine)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Default for OracleContract {
    fn default() -> Self {
        Self::new()
    }
}
