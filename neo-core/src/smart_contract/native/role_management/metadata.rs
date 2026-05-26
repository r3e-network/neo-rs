use super::RoleManagement;
use crate::hardfork::Hardfork;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::manifest::ContractEventDescriptor;
use crate::smart_contract::native::metadata_macros::event_descriptor;
use crate::smart_contract::native::method_macros::{
    neo_native_method_dispatch, neo_native_method_metadata,
};
use crate::smart_contract::native::NativeMethod;

macro_rules! role_management_method_table {
    ($callback:ident; $($args:tt)*) => {
        $callback! {
            $($args)*
            ;
            {
                safe "getDesignatedByRole", fee = Self::CPU_FEE, flags = [READ_STATES], params = [Integer, Integer], returns = Array, names = ["role", "index"] => engine get_designated_by_role;
                unsafe "designateAsRole", fee = Self::CPU_FEE, flags = [STATES, ALLOW_NOTIFY], params = [Integer, Array], returns = Void, names = ["role", "nodes"] => engine designate_as_role;
            }
        }
    };
}

impl RoleManagement {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        role_management_method_table!(neo_native_method_metadata;)
    }

    pub(super) fn dispatch_method(
        &self,
        engine: &mut crate::smart_contract::ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> crate::error::CoreResult<Vec<u8>> {
        role_management_method_table!(
            neo_native_method_dispatch;
            self,
            engine,
            method,
            args,
            aliases = [],
            unknown = |method| crate::error::CoreError::native_contract(format!("Unknown method: {}", method))
        )
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
