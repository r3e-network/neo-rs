use alloc::rc::Rc;
use neo_vm::{OpCode, ScriptBuilder, StackItem, VMState};
use neo_vm::stackitem_type::{ExecutionContext};
use std::convert::TryInto;
use std::collections::HashMap;
use neo_type::{Script, H160};
use crate::contract::CallFlags;
use crate::cryptography::{Crypto, ECCurve, ECPoint};
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::execution_context_state::ExecutionContextState;
use crate::neo_contract::iinteroperable::IInteroperable;
use crate::neo_contract::manifest::contract_abi::ContractAbi;
use crate::neo_contract::trigger_type::TriggerType;
use crate::network::payloads::IVerifiable;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
/// A helper module related to smart contracts.
    use super::*;

    /// The maximum GAS that can be consumed when `verify_witnesses` is called.
    /// The unit is datoshi, 1 datoshi = 1e-8 GAS
    pub const MAX_VERIFICATION_GAS: i64 = 1_50000000;

    /// Calculates the verification fee for a signature address.
    /// In the unit of datoshi, 1 datoshi = 1e-8 GAS
    pub fn signature_contract_cost() -> i64 {
        ApplicationEngine::op_code_price_table(OpCode::PushData1 as u8) * 2 +
            ApplicationEngine::op_code_price_table(OpCode::Syscall as u8) +
            ApplicationEngine::check_sig_price()
    }

    /// Calculates the verification fee for a multi-signature address.
    /// In the unit of datoshi, 1 datoshi = 1e-8 GAS
    pub fn multi_signature_contract_cost(m: i32, n: i32) -> i64 {
        let mut fee = ApplicationEngine::op_code_price_table(OpCode::PushData1 as u8) * (m + n) as i64;
        
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
        let _ = abi.events.iter().map(|e| (e.name.clone(), e)).collect::<HashMap<_, _>>(); // Check the uniqueness of the event names.
    }

    /// Computes the hash of a deployed contract.
    pub fn get_contract_hash(sender: &H160, nef_check_sum: u32, name: &str) -> H160 {
        let mut sb = ScriptBuilder::new();
        sb.emit(OpCode::Abort);
        sb.emit_push(sender);
        sb.emit_push(nef_check_sum);
        sb.emit_push(name);

        H160::from(&sb.to_array())
    }

    /// Gets the script hash of the specified `ExecutionContext`.
    pub fn get_script_hash(context: &ExecutionContext) -> H160 {
        context.get_state::<ExecutionContextState>().script_hash
    }

    /// Determines whether the specified contract is a multi-signature contract.
    pub fn is_multi_sig_contract(script: &[u8]) -> bool {
        is_multi_sig_contract_with_params(script, &mut 0, &mut 0, None)
    }

    /// Determines whether the specified contract is a multi-signature contract.
    pub fn is_multi_sig_contract_with_m_n(script: &[u8], m: &mut i32, n: &mut i32) -> bool {
        is_multi_sig_contract_with_params(script, m, n, None)
    }

    /// Determines whether the specified contract is a multi-signature contract.
    pub fn is_multi_sig_contract_with_points(script: &[u8], m: &mut i32, points: &mut Vec<ECPoint>) -> bool {
        let mut n = 0;
        is_multi_sig_contract_with_params(script, m, &mut n, Some(points))
    }

    fn is_multi_sig_contract_with_params(script: &[u8], m: &mut i32, n: &mut i32, points: Option<&mut Vec<ECPoint>>) -> bool {
        if script.len() < 42 { return false; }
        let mut i = 0;

        // Check if the script starts with PUSHINT instructions for m and n
        if let Some(push_m) = OpCode::try_from(script[i]).ok().filter(|&op| op.is_push_number()) {
            *m = push_m.get_number() as i32;
            i += 1;
        } else {
            return false;
        }

        if let Some(push_n) = OpCode::try_from(script[i]).ok().filter(|&op| op.is_push_number()) {
            *n = push_n.get_number() as i32;
            i += 1;
        } else {
            return false;
        }

        // Validate m and n
        if *m <= 0 || *n < *m || *n > 1024 { return false; }

        // Check for n public keys
        let mut public_keys = Vec::new();
        for _ in 0..*n {
            if script[i] != OpCode::PushData1 as u8 || script[i + 1] != 33 { return false; }
            i += 2;
            public_keys.push(&script[i..i + 33]);
            i += 33;
        }

        // Check for SYSCALL instruction
        if script[i] != OpCode::Syscall as u8 { return false; }
        i += 1;
        if u32::from_le_bytes(script[i..i + 4].try_into().unwrap()) != ApplicationEngine::SYSTEM_CRYPTO_CHECK_MULTISIG { return false; }
        i += 4;

        // Check if there's any remaining script
        if i != script.len() { return false; }

        // If points vector is provided, populate it
        if let Some(points) = points {
            points.clear();
            for key in public_keys {
                if let Ok(point) = ECPoint::decode_point(key, Rc::clone(&ECCurve::secp256r1())) {
                    points.push(point);
                } else {
                    return false;
                }
            }
        }

        true
    }

    /// Determines whether the specified contract is a signature contract.
    pub fn is_signature_contract(script: &[u8]) -> bool {
        if script.len() != 40 { return false; }
        if script[0] != OpCode::PushData1 as u8
            || script[1] != 33
            || script[35] != OpCode::Syscall as u8
            || u32::from_le_bytes(script[36..40].try_into().unwrap()) != ApplicationEngine::SYSTEM_CRYPTO_CHECK_SIG
        {
            return false;
        }
        true
    }

    /// Determines whether the specified contract is a standard contract.
    pub fn is_standard_contract(script: &[u8]) -> bool {
        is_signature_contract(script) || is_multi_sig_contract(script)
    }

    /// Convert the `StackItem` to an `IInteroperable`.
    pub fn to_interoperable<T: IInteroperable + Default>(item: &StackItem) -> T {
        let mut t = T::default();
        t.from_stack_item(item);
        t
    }

    /// Computes the hash of the specified script.
    pub fn to_script_hash(script: &[u8]) -> H160 {
        H160::from_slice(&Crypto::hash160(script))
    }

    /// Verifies the witnesses of the specified `IVerifiable`.
    pub fn verify_witnesses(verifiable: &dyn IVerifiable<Error=()>, settings: &ProtocolSettings, snapshot: &dyn DataCache, datoshi: i64) -> bool {
        let hashes = verifiable.get_script_hashes();
        if hashes.len() != verifiable.get_witnesses().len() {
            return false;
        }
        let mut fee = 0;
        for (script, witness) in hashes.iter().zip(verifiable.get_witnesses()) {
            if !verify_witness(verifiable, settings, snapshot, script, witness, datoshi, &mut fee) {
                return false;
            }
        }
        fee <= settings.exec_fee_factor * datoshi
    }

    /// Verifies a single witness.
    pub(crate) fn verify_witness(verifiable: &dyn IVerifiable<Error=()>, settings: &ProtocolSettings, snapshot: &DataCache, hash: &H160, witness: &Witness, datoshi: i64, fee: &mut i64) -> bool {
        let verification_script = witness.verification_script();
        let invocation_script = witness.invocation_script();
        
        if verification_script.is_empty() {
            let contract_state = snapshot.get_contract_state(hash).ok_or(false)?;
            if contract_state.nef().script().is_empty() {
                return false;
            }
        } else if to_script_hash(verification_script) != *hash {
            return false;
        }

        let engine = ApplicationEngine::new(
            TriggerType::VERIFICATION,
            verifiable,
            snapshot,
            datoshi,
            settings,
        );

        engine.load_script(invocation_script, CallFlags::None);
        engine.load_script(verification_script, CallFlags::None);

        let result = engine.execute();
        *fee += engine.gas_consumed();

        if result == VMState::Fault {
            return false;
        }

        if engine.result_stack.len() != 1 {
            return false;
        }

        engine.result_stack[0].get_boolean()
    }
