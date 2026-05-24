use super::ContractManagement;
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractParameterDefinition};
use crate::smart_contract::native::method_macros::neo_native_methods;
use crate::smart_contract::native::NativeMethod;
use crate::smart_contract::ContractParameterType;

impl ContractManagement {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        neo_native_methods![
            safe "getContract", fee = 1 << 15, flags = [READ_STATES], params = [Hash160], returns = Array, names = ["hash"];
            unsafe "deploy", fee = 0, flags = [STATES, ALLOW_NOTIFY], params = [ByteArray, ByteArray], returns = Array, names = ["nefFile", "manifest"];
            unsafe "deploy", fee = 0, flags = [STATES, ALLOW_NOTIFY], params = [ByteArray, ByteArray, Any], returns = Array, names = ["nefFile", "manifest", "data"];
            unsafe "update", fee = 0, flags = [STATES, ALLOW_NOTIFY], params = [ByteArray, ByteArray], returns = Void, names = ["nefFile", "manifest"];
            unsafe "update", fee = 0, flags = [STATES, ALLOW_NOTIFY], params = [ByteArray, ByteArray, Any], returns = Void, names = ["nefFile", "manifest", "data"];
            unsafe "destroy", fee = 1 << 15, flags = [STATES, ALLOW_NOTIFY], params = [], returns = Void;
            safe "getMinimumDeploymentFee", fee = 1 << 15, flags = [READ_STATES], params = [], returns = Integer;
            unsafe "setMinimumDeploymentFee", fee = 1 << 15, flags = [STATES], params = [Integer], returns = Void, names = ["value"];
            safe "hasMethod", fee = 1 << 15, flags = [READ_STATES], params = [Hash160, String, Integer], returns = Boolean, names = ["hash", "method", "pcount"];
            safe "getContractById", fee = 1 << 15, flags = [READ_STATES], params = [Integer], returns = Array, names = ["id"];
            safe "isContract", fee = 1 << 14, flags = [READ_STATES], params = [Hash160], returns = Boolean, active = HfEchidna, names = ["hash"];
            safe "getContractHashes", fee = 1 << 15, flags = [READ_STATES], params = [], returns = InteropInterface;
        ]
    }

    pub(super) fn event_descriptors() -> Vec<ContractEventDescriptor> {
        vec![
            Self::event_descriptor("Deploy"),
            Self::event_descriptor("Update"),
            Self::event_descriptor("Destroy"),
        ]
    }

    fn event_descriptor(name: &str) -> ContractEventDescriptor {
        ContractEventDescriptor::new(
            name.to_string(),
            vec![ContractParameterDefinition::new(
                "Hash".to_string(),
                ContractParameterType::Hash160,
            )
            .expect("contract management event hash parameter")],
        )
        .expect("contract management event descriptor")
    }
}
