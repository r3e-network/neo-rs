// Application Engine related modules
pub mod application_engine;
pub mod application_engine_contract;
pub mod application_engine_crypto;
pub mod application_engine_iterator;
pub mod application_engine_op_code_prices;
pub mod application_engine_runtime;
pub mod application_engine_storage;

// Contract related modules
pub mod contract;
pub mod contract_basic_method;
pub mod contract_parameter;
pub mod contract_parameter_type;
pub mod contract_parameters_context;
pub mod contract_state;
pub mod contract_task;
pub mod contract_task_awaiter;
pub mod contract_task_method_builder;
pub mod deployed_contract;

// Interop related modules
pub mod iinteroperable;
pub mod iinteroperable_verifiable;
pub mod interop_descriptor;
pub mod interop_parameter_descriptor;

// Storage related modules
pub mod storage_context;
pub mod storage_item;
pub mod storage_key;

// Serialization related modules
pub mod binary_serializer;
pub mod json_serializer;

// Other modules
pub mod call_flags;
pub mod method_token;
pub mod idiagnostic;
pub mod key_builder;
pub mod log_event_args;
pub mod max_length_attribute;
pub mod nef_file;
pub mod notify_event_args;
pub mod trigger_type;
pub mod validator_attribute;
pub mod execution_context_state;
pub mod find_options;
pub mod manifest;
pub mod helper;
pub mod iapplication_engine_provider;
mod native_contract;
mod iterators;

// Example function (you may want to remove this if it's not needed)
pub fn add(left: usize, right: usize) -> usize {
    left + right
}

// Tests (you may want to remove this if it's not needed)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
