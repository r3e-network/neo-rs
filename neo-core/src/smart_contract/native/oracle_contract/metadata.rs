use super::OracleContract;
use crate::smart_contract::manifest::ContractEventDescriptor;
use crate::smart_contract::native::metadata_macros::event_descriptor;
use crate::smart_contract::native::method_macros::neo_native_contract_methods;

macro_rules! oracle_method_table {
    ([$($callback:tt)+]; $($args:tt)*) => {
        $($callback)+! {
            $($args)*
            ;
            {
                unsafe "request", fee = 0, flags = [STATES, ALLOW_NOTIFY], params = [String, String, String, Any, Integer], returns = Void, names = ["url", "filter", "callback", "userData", "gasForResponse"] => engine request;
                safe "getPrice", fee = 1 << 15, flags = [READ_STATES], params = [], returns = Integer => engine invoke_get_price;
                unsafe "setPrice", fee = 1 << 15, flags = [STATES], params = [Integer], returns = Void, names = ["price"] => engine set_price;
                unsafe "finish", fee = 0, flags = [STATES, ALLOW_CALL, ALLOW_NOTIFY], params = [], returns = Void => engine invoke_finish;
                safe "verify", fee = 1 << 15, flags = [], params = [], returns = Boolean => engine invoke_verify;
            }
        }
    };

    ($callback:ident; $($args:tt)*) => {
        oracle_method_table!([$callback]; $($args)*)
    };
}

neo_native_contract_methods!(OracleContract, table = oracle_method_table);

impl OracleContract {
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
