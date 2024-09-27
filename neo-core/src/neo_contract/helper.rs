use neo_cryptography::{Crypto, ECC};
use neo_network_p2p::payloads::Witness;
use neo_persistence::DataCache;
use neo_smart_contract::manifest::{ContractAbi, ContractMethodDescriptor};
use neo_smart_contract::native::NativeContract;
use neo_vm::{OpCode, Script, ScriptBuilder, StackItem, VMState};
use neo_vm::stackitem_type::{ExecutionContext, ExecutionContextState};
use std::convert::TryInto;

/// A helper module related to smart contracts.
pub mod helper {
    use NeoRust::builder::ScriptBuilder;
    use neo_vm::vm::script::Script;
    use crate::neo_contract::application_engine::ApplicationEngine;
    use crate::neo_contract::execution_context_state::ExecutionContextState;
    use crate::neo_contract::manifest::contract_abi::ContractAbi;
    use neo_type::H160;
    use super::*;

    /// The maximum GAS that can be consumed when `verify_witnesses` is called.
    /// The unit is datoshi, 1 datoshi = 1e-8 GAS
    pub const MAX_VERIFICATION_GAS: i64 = 1_50000000;

    /// Calculates the verification fee for a signature address.
    /// In the unit of datoshi, 1 datoshi = 1e-8 GAS
    pub fn signature_contract_cost() -> i64 {
        ApplicationEngine::op_code_price_table(OpCode::PUSHDATA1 as u8) * 2 +
            ApplicationEngine::op_code_price_table(OpCode::SYSCALL as u8) +
            ApplicationEngine::check_sig_price()
    }

    /// Calculates the verification fee for a multi-signature address.
    /// In the unit of datoshi, 1 datoshi = 1e-8 GAS
    pub fn multi_signature_contract_cost(m: i32, n: i32) -> i64 {
        let mut fee = ApplicationEngine::op_code_price_table(OpCode::PUSHDATA1 as u8) * (m + n) as i64;
        
        let mut sb = ScriptBuilder::new();
        fee += ApplicationEngine::op_code_price_table(sb.emit_push(m).to_array()[0] as u8);
        
        let mut sb = ScriptBuilder::new();
        fee += ApplicationEngine::op_code_price_table(sb.emit_push(n).to_array()[0] as u8);
        
        fee += ApplicationEngine::op_code_price_table(OpCode::SYSCALL as u8);
        fee += ApplicationEngine::check_sig_price() * n as i64;
        fee
    }

    /// Check the correctness of the script and ABI.
    pub fn check(script: &[u8], abi: &ContractAbi) {
        check_script(Script::new(script.to_vec(), true), abi);
    }

    /// Check the correctness of the script and ABI.
    /// Note: The `Script` passed to this method should be constructed with strict mode.
    pub fn check_script(script: Script, abi: &ContractAbi) {
        for method in &abi.methods {
            script.get_instruction(method.offset);
        }
        abi.get_method("", 0); // Trigger the construction of ContractAbi.method_dictionary to check the uniqueness of the method names.
        let _ = abi.events.iter().map(|e| (e.name.clone(), e)).collect::<std::collections::HashMap<_, _>>(); // Check the uniqueness of the event names.
    }

    /// Computes the hash of a deployed contract.
    pub fn get_contract_hash(sender: &H160, nef_check_sum: u32, name: &str) -> H160 {
        let mut sb = ScriptBuilder::new();
        sb.emit(OpCode::ABORT);
        sb.emit_push(sender);
        sb.emit_push(nef_check_sum);
        sb.emit_push(name);

        H160::from_script_hash(&sb.to_array())
    }

    /// Gets the script hash of the specified `ExecContext`.
    pub fn get_script_hash(context: &ExecContext) -> H160 {
        context.get_state::<ExecutionContextState>().script_hash
    }

    // ... (remaining functions would be implemented similarly)
}
