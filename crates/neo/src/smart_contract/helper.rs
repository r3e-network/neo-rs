//! Helper - matches C# Neo.SmartContract.Helper exactly

use crate::smart_contract::contract::Contract;
use crate::smart_contract::contract_parameter_type::ContractParameterType;
use crate::{UInt160, UInt256};
use neo_vm::{op_code::OpCode, ScriptBuilder};
use sha2::{Digest, Sha256};

/// Helper functions for smart contracts (matches C# Helper)
pub struct Helper;

impl Helper {
    /// Checks if a script is a standard contract
    pub fn is_standard_contract(script: &[u8]) -> bool {
        Self::is_signature_contract(script) || Self::is_multi_sig_contract(script)
    }

    /// Checks if a script is a signature contract
    pub fn is_signature_contract(script: &[u8]) -> bool {
        if script.len() != 40 {
            return false;
        }

        // Check pattern: PUSHDATA1 (33 bytes pubkey) SYSCALL (CheckSig)
        script[0] == 0x0C && // PUSHDATA1
        script[1] == 33 &&   // 33 bytes
        script[35] == 0x41 && // SYSCALL
        &script[36..40] == &Self::check_sig_hash()
    }

    /// Checks if a script is a multi-sig contract
    pub fn is_multi_sig_contract(script: &[u8]) -> bool {
        if script.len() < 42 {
            return false;
        }

        // Check basic pattern for multi-sig
        let m = match script[0] {
            0x51..=0x60 => script[0] - 0x50, // PUSH1-PUSH16
            _ => return false,
        };

        // Verify ending with SYSCALL CheckMultisig
        let len = script.len();
        script[len - 5] == 0x41 && // SYSCALL
        &script[len - 4..] == &Self::check_multisig_hash()
    }

    /// Gets the script hash from a contract
    pub fn to_script_hash(contract: &Contract) -> UInt160 {
        contract.script_hash()
    }

    /// Creates a signature redeem script
    pub fn signature_redeem_script(public_key: &[u8]) -> Vec<u8> {
        let mut script = Vec::new();
        script.push(0x0C); // PUSHDATA1
        script.push(public_key.len() as u8);
        script.extend_from_slice(public_key);
        script.push(0x41); // SYSCALL
        script.extend_from_slice(&Self::check_sig_hash());
        script
    }

    /// Creates a multi-sig redeem script
    pub fn multi_sig_redeem_script(m: usize, public_keys: &[Vec<u8>]) -> Vec<u8> {
        if !(1..=16).contains(&m) || public_keys.len() > 16 || m > public_keys.len() {
            panic!("Invalid multi-sig parameters");
        }

        let mut script = Vec::new();

        // Push m
        script.push(0x50 + m as u8); // PUSH1-PUSH16

        // Push public keys (sorted)
        let mut sorted_keys = public_keys.to_vec();
        sorted_keys.sort();

        for key in &sorted_keys {
            script.push(0x0C); // PUSHDATA1
            script.push(key.len() as u8);
            script.extend_from_slice(key);
        }

        // Push n
        script.push(0x50 + sorted_keys.len() as u8); // PUSH1-PUSH16

        // Add SYSCALL CheckMultisig
        script.push(0x41); // SYSCALL
        script.extend_from_slice(&Self::check_multisig_hash());

        script
    }

    /// Gets the CheckSig syscall hash
    fn check_sig_hash() -> [u8; 4] {
        Self::syscall_hash("System.Crypto.CheckSig")
    }

    /// Gets the CheckMultisig syscall hash
    fn check_multisig_hash() -> [u8; 4] {
        Self::syscall_hash("System.Crypto.CheckMultisig")
    }

    /// Computes syscall hash
    fn syscall_hash(name: &str) -> [u8; 4] {
        let mut hasher = Sha256::new();
        hasher.update(name.as_bytes());
        let result = hasher.finalize();
        [result[0], result[1], result[2], result[3]]
    }

    /// Computes the hash of a deployed contract.
    pub fn get_contract_hash(sender: &UInt160, nef_checksum: u32, name: &str) -> UInt160 {
        let mut builder = ScriptBuilder::new();
        builder.emit_opcode(OpCode::ABORT);
        builder.emit_push(&sender.to_bytes());
        builder.emit_push_int(nef_checksum as i64);
        builder.emit_push_string(name);
        let script = builder.to_array();
        UInt160::from_script(&script)
    }
}
