use super::RoleManagement;
use crate::hardfork::Hardfork;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::manifest::ContractEventDescriptor;
use crate::smart_contract::native::metadata_macros::event_descriptor;
use crate::smart_contract::native::method_macros::neo_native_methods;
use crate::smart_contract::native::NativeMethod;

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
        if settings.is_hardfork_enabled(Hardfork::HfEchidna, block_height) {
            return vec![event_descriptor!(
                "Designation",
                ["Role" => Integer, "BlockIndex" => Integer, "Old" => Array, "New" => Array]
            )];
        }

        vec![event_descriptor!(
            "Designation",
            ["Role" => Integer, "BlockIndex" => Integer]
        )]
    }
}
