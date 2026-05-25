use super::OracleContract;
use crate::smart_contract::manifest::ContractEventDescriptor;
use crate::smart_contract::native::metadata_macros::event_descriptor;
use crate::smart_contract::native::method_macros::neo_native_methods;
use crate::smart_contract::native::NativeMethod;

impl OracleContract {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        neo_native_methods![
            unsafe "request", fee = 0, flags = [STATES, ALLOW_NOTIFY], params = [String, String, String, Any, Integer], returns = Void, names = ["url", "filter", "callback", "userData", "gasForResponse"];
            safe "getPrice", fee = 1 << 15, flags = [READ_STATES], params = [], returns = Integer;
            unsafe "setPrice", fee = 1 << 15, flags = [STATES], params = [Integer], returns = Void, names = ["price"];
            unsafe "finish", fee = 0, flags = [STATES, ALLOW_CALL, ALLOW_NOTIFY], params = [], returns = Void;
            safe "verify", fee = 1 << 15, flags = [], params = [], returns = Boolean;
        ]
    }

    pub(super) fn event_descriptors() -> Vec<ContractEventDescriptor> {
        vec![
            event_descriptor!(
                "OracleRequest",
                [
                    "Id" => Integer,
                    "RequestContract" => Hash160,
                    "Url" => String,
                    "Filter" => String,
                ]
            ),
            event_descriptor!(
                "OracleResponse",
                ["Id" => Integer, "OriginalTx" => Hash256]
            ),
        ]
    }
}
