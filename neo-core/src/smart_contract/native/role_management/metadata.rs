use super::RoleManagement;
use crate::hardfork::Hardfork;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractParameterDefinition};
use crate::smart_contract::native::method_macros::neo_native_methods;
use crate::smart_contract::native::NativeMethod;
use crate::smart_contract::ContractParameterType;

impl RoleManagement {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        neo_native_methods![
            safe "getDesignatedByRole", fee = Self::CPU_FEE, flags = [READ_STATES], params = [Integer, Integer], returns = Array, names = ["role", "index"];
            unsafe "designateAsRole", fee = Self::CPU_FEE, flags = [STATES, ALLOW_NOTIFY], params = [Integer, Array], returns = Void, names = ["role", "nodes"];
        ]
    }

    pub(super) fn event_descriptors(
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        let mut parameters = vec![
            ContractParameterDefinition::new("Role".to_string(), ContractParameterType::Integer)
                .expect("Designation.Role"),
            ContractParameterDefinition::new(
                "BlockIndex".to_string(),
                ContractParameterType::Integer,
            )
            .expect("Designation.BlockIndex"),
        ];

        if settings.is_hardfork_enabled(Hardfork::HfEchidna, block_height) {
            parameters.push(
                ContractParameterDefinition::new("Old".to_string(), ContractParameterType::Array)
                    .expect("Designation.Old"),
            );
            parameters.push(
                ContractParameterDefinition::new("New".to_string(), ContractParameterType::Array)
                    .expect("Designation.New"),
            );
        }

        vec![
            ContractEventDescriptor::new("Designation".to_string(), parameters)
                .expect("Designation event descriptor"),
        ]
    }
}
