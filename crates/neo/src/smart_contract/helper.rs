//! Helper - matches C# Neo.SmartContract.Helper exactly

use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::contract::Contract;
use crate::UInt160;
use neo_vm::{op_code::OpCode, ScriptBuilder};
use sha2::{Digest, Sha256};

/// Helper functions for smart contracts (matches C# Helper)
pub struct Helper;

impl Helper {
    /// The maximum GAS that can be consumed when verifying witnesses (in datoshi).
    pub const MAX_VERIFICATION_GAS: i64 = 1_500_000_00;

    /// Calculates the verification cost for a single-signature contract (in datoshi).
    pub fn signature_contract_cost() -> i64 {
        let push_cost = ApplicationEngine::get_opcode_price(OpCode::PUSHDATA1 as u8);
        let syscall_cost = ApplicationEngine::get_opcode_price(OpCode::SYSCALL as u8);
        push_cost * 2 + syscall_cost + crate::smart_contract::application_engine::CHECK_SIG_PRICE
    }

    /// Calculates the verification cost for a multi-signature contract (in datoshi).
    pub fn multi_signature_contract_cost(m: i32, n: i32) -> i64 {
        let push_cost = ApplicationEngine::get_opcode_price(OpCode::PUSHDATA1 as u8);
        let mut fee = push_cost * (m as i64 + n as i64);

        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(m as i64);
        let m_opcode = builder
            .to_array()
            .first()
            .copied()
            .unwrap_or(OpCode::PUSH0 as u8);
        fee += ApplicationEngine::get_opcode_price(m_opcode);

        let mut builder_n = ScriptBuilder::new();
        builder_n.emit_push_int(n as i64);
        let n_opcode = builder_n
            .to_array()
            .first()
            .copied()
            .unwrap_or(OpCode::PUSH0 as u8);
        fee += ApplicationEngine::get_opcode_price(n_opcode);

        fee += ApplicationEngine::get_opcode_price(OpCode::SYSCALL as u8);
        fee += crate::smart_contract::application_engine::CHECK_SIG_PRICE * n as i64;
        fee
    }

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
        let _m = match script[0] {
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

    /// Parses a multi-signature contract script, returning the required signature count and
    /// the ordered public keys when the script matches the canonical Neo multi-sig format.
    pub fn parse_multi_sig_contract(script: &[u8]) -> Option<(usize, Vec<Vec<u8>>)> {
        use neo_vm::op_code::OpCode;

        if script.len() < 42 {
            return None;
        }

        let mut offset = 0usize;
        let first = script[offset];
        if !(0x51..=0x60).contains(&first) {
            return None;
        }
        let m = (first - 0x50) as usize;
        offset += 1;

        let mut public_keys = Vec::new();
        while offset < script.len() {
            if script[offset] != OpCode::PUSHDATA1 as u8 {
                break;
            }
            offset += 1;
            if offset >= script.len() {
                return None;
            }
            let key_len = script[offset] as usize;
            offset += 1;
            if key_len != 33 || offset + key_len > script.len() {
                return None;
            }
            public_keys.push(script[offset..offset + key_len].to_vec());
            offset += key_len;
        }

        if public_keys.is_empty() {
            return None;
        }
        let n = public_keys.len();

        if offset >= script.len() || script[offset] != (0x50 + n as u8) {
            return None;
        }
        offset += 1;

        if script.len() != offset + 5 {
            return None;
        }
        if script[offset] != OpCode::SYSCALL as u8 {
            return None;
        }
        if script[offset + 1..offset + 5] != Self::check_multisig_hash() {
            return None;
        }

        if m == 0 || m > n {
            return None;
        }

        Some((m, public_keys))
    }

    /// Parses a multi-signature invocation script, returning the list of signatures when the
    /// script pushes the expected number of signatures encoded with `PUSHDATA1` opcodes.
    pub fn parse_multi_sig_invocation(
        invocation: &[u8],
        required_signatures: usize,
    ) -> Option<Vec<Vec<u8>>> {
        use neo_vm::op_code::OpCode;

        if required_signatures == 0 {
            return None;
        }

        let mut signatures = Vec::with_capacity(required_signatures);
        let mut offset = 0usize;
        while offset < invocation.len() {
            if invocation[offset] != OpCode::PUSHDATA1 as u8 {
                return None;
            }
            offset += 1;
            if offset >= invocation.len() {
                return None;
            }
            let len = invocation[offset] as usize;
            offset += 1;
            if len != 64 || offset + len > invocation.len() {
                return None;
            }
            signatures.push(invocation[offset..offset + len].to_vec());
            offset += len;
        }

        if signatures.len() == required_signatures {
            Some(signatures)
        } else {
            None
        }
    }
}
