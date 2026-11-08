//! Helper - matches C# Neo.SmartContract.Helper exactly
//!
//! This module is the canonical home of the script/multisig primitives
//! (`signature_contract_cost`, `is_signature_contract`, `parse_multi_sig_contract`,
//! `get_contract_hash`, ...). `neo-core::smart_contract::helper::Helper` is a thin
//! delegating shim over these, so the logic has a single source of truth.
//!
//! # Not migrated here: `verify_witness` / `verify_witnesses`
//!
//! The engine-driven witness verification entry points stay in `neo-core` because
//! they need types that sit *above* this crate in the dependency graph:
//!
//! - `ContractManagement` lives in `neo-native-contracts`, which depends on
//!   `neo-smart-contract` (importing it here would be a cycle).
//! - `VerifiableExt` (`script_hashes_for_verifying`) lives in `neo-ledger`, which
//!   also depends on `neo-smart-contract` (same cycle).
//! - `default_native_services()` comes from `neo-core`'s native composition layer.
//!
//! Trait bounds cannot break these cycles: the bound would still have to name a
//! trait owned by a downstream crate. Feature-gating does not help either, since
//! the cycle exists at the Cargo graph level regardless of feature selection.
//! Everything those functions call that *is* layer-safe (`is_signature_contract`,
//! `multi_sig_redeem_script`, `MAX_VERIFICATION_GAS`, ...) already lives here.

use crate::application_engine::ApplicationEngine;
use crate::contract_state::ContractState;
use crate::error::{CoreError, CoreResult};
use neo_crypto::ECPoint;
use neo_primitives::UInt160;
use neo_vm::OpCode;
use neo_vm::ScriptBuilder;

/// Helper functions for smart contracts (matches C# Helper)
pub struct Helper;

impl Helper {
    /// The maximum GAS that can be consumed when verifying witnesses (in datoshi).
    pub const MAX_VERIFICATION_GAS: i64 = 150_000_000;

    /// Calculates the verification cost for a single-signature contract (in datoshi).
    pub fn signature_contract_cost() -> i64 {
        let push_cost = ApplicationEngine::get_opcode_price(OpCode::PUSHDATA1.byte());
        let syscall_cost = ApplicationEngine::get_opcode_price(OpCode::SYSCALL.byte());
        push_cost
            .saturating_mul(2)
            .saturating_add(syscall_cost)
            .saturating_add(crate::application_engine::CHECK_SIG_PRICE)
    }

    /// Calculates the verification cost for a multi-signature contract (in datoshi).
    pub fn multi_signature_contract_cost(m: i32, n: i32) -> i64 {
        let push_cost = ApplicationEngine::get_opcode_price(OpCode::PUSHDATA1.byte());
        let m_plus_n = (m as i64).saturating_add(n as i64);
        let mut fee = push_cost.saturating_mul(m_plus_n);

        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(m as i64);
        let m_opcode = builder
            .to_array()
            .first()
            .copied()
            .unwrap_or(OpCode::PUSH0.byte());
        fee = fee.saturating_add(ApplicationEngine::get_opcode_price(m_opcode));

        let mut builder_n = ScriptBuilder::new();
        builder_n.emit_push_int(n as i64);
        let n_opcode = builder_n
            .to_array()
            .first()
            .copied()
            .unwrap_or(OpCode::PUSH0.byte());
        fee = fee.saturating_add(ApplicationEngine::get_opcode_price(n_opcode));

        fee = fee.saturating_add(ApplicationEngine::get_opcode_price(OpCode::SYSCALL.byte()));
        let check_sig_cost = crate::application_engine::CHECK_SIG_PRICE.saturating_mul(n as i64);
        fee = fee.saturating_add(check_sig_cost);
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
        script[0] == OpCode::PUSHDATA1.byte() &&
        script[1] == 33 &&   // 33 bytes
        script[35] == OpCode::SYSCALL.byte() &&
        script[36..40] == Self::check_sig_hash()
    }

    /// Maximum number of public keys allowed in a multi-sig contract (C# neo v3.10.2).
    pub const MAX_PUBLIC_KEYS_PER_MULTISIG: usize = 1024;

    /// Checks if a script is a multi-sig contract.
    ///
    /// This is the only classification path. C# v3.10.2 decodes every public key
    /// before any `IsMultiSigContract` overload returns true; a shape-only check
    /// let a malformed key through the `bool` overload.
    pub fn is_multi_sig_contract(script: &[u8]) -> bool {
        Self::parse_multi_sig_contract(script).is_some()
    }

    /// Gets the script hash from a contract
    pub fn to_script_hash(contract: &ContractState) -> UInt160 {
        contract.hash
    }

    /// Creates a signature redeem script
    pub fn signature_redeem_script(public_key: &[u8]) -> Vec<u8> {
        let mut script = Vec::new();
        script.push(OpCode::PUSHDATA1.byte());
        script.push(public_key.len() as u8);
        script.extend_from_slice(public_key);
        script.push(OpCode::SYSCALL.byte());
        script.extend_from_slice(&Self::check_sig_hash());
        script
    }

    /// Creates a multi-sig redeem script.
    ///
    /// # Errors
    ///
    /// Returns `CoreError` if:
    /// - `m` is not in range `1..=1024`
    /// - `public_keys.len()` exceeds 1024
    /// - `m` exceeds `public_keys.len()`
    /// - any key is not a valid secp256r1 point
    pub fn try_multi_sig_redeem_script(m: usize, public_keys: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        let max = Self::MAX_PUBLIC_KEYS_PER_MULTISIG;
        if !(1..=max).contains(&m) || public_keys.len() > max || m > public_keys.len() {
            return Err(CoreError::invalid_operation(format!(
                "Invalid multi-sig parameters: m={}, n={}",
                m,
                public_keys.len()
            )));
        }

        let mut points = Vec::with_capacity(public_keys.len());
        for key in public_keys {
            let point = ECPoint::from_bytes(key)
                .map_err(|e| CoreError::invalid_operation(format!("Invalid public key: {e}")))?;
            points.push(point);
        }

        // Build the multi-sig redeem script directly
        Self::create_multi_sig_script_from_points(m, &points)
    }

    /// Internal method to create multi-sig script from ECPoints
    fn create_multi_sig_script_from_points(m: usize, points: &[ECPoint]) -> CoreResult<Vec<u8>> {
        use neo_vm::ScriptBuilder;

        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(m as i64);

        for point in points {
            builder.emit_push(&point.to_bytes());
        }

        builder.emit_push_int(points.len() as i64);
        let hash_bytes = Self::check_multisig_hash();
        let hash = u32::from_le_bytes(hash_bytes);
        builder.emit_syscall_hash(hash);

        Ok(builder.to_array())
    }

    /// Creates a multi-sig redeem script (panics on invalid input).
    ///
    /// Prefer `try_multi_sig_redeem_script` for fallible construction.
    #[inline]
    pub fn multi_sig_redeem_script(m: usize, public_keys: &[Vec<u8>]) -> Vec<u8> {
        Self::try_multi_sig_redeem_script(m, public_keys).expect("Invalid multi-sig parameters")
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
        neo_vm::interop_hash(name).to_le_bytes()
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
    ///
    /// Mirrors C# `Helper.IsMultiSigContract` (v3.10.2): m/n accept `PUSHINT8`/`PUSHINT16`
    /// as well as `PUSH1..PUSH16`, both are bounded by 1..=1024, every 33-byte key must
    /// decode as a secp256r1 point, and the tail must be `n`-push, `SYSCALL`, `CheckMultisig`.
    /// Decoding runs even when the caller only needs a boolean.
    pub fn parse_multi_sig_contract(script: &[u8]) -> Option<(usize, Vec<Vec<u8>>)> {
        use neo_vm::OpCode;

        let max_keys = Self::MAX_PUBLIC_KEYS_PER_MULTISIG;

        if script.len() < 42 {
            return None;
        }

        let (m, mut offset) = Self::read_multisig_push(script, 0)?;
        if !(1..=max_keys).contains(&m) {
            return None;
        }

        let mut public_keys = Vec::new();
        let mut n = 0usize;
        while offset < script.len() && script[offset] == OpCode::PUSHDATA1.byte() {
            // C# requires one byte beyond this push so the next opcode read stays in bounds.
            if script.len() <= offset + 35 {
                return None;
            }
            if script[offset + 1] != 33 {
                return None;
            }
            let key = &script[offset + 2..offset + 35];
            // C# decodes each point on Secp256r1; arbitrary data rejects the script.
            ECPoint::from_bytes_with_curve(neo_crypto::ECCurve::Secp256r1, key).ok()?;
            public_keys.push(key.to_vec());
            offset += 35;
            n += 1;
        }

        if n < m || n > max_keys {
            return None;
        }

        let (pushed_n, offset_after_n) = Self::read_multisig_push(script, offset)?;
        if pushed_n != n {
            return None;
        }
        offset = offset_after_n;

        if script.len() != offset + 5 {
            return None;
        }
        if script[offset] != OpCode::SYSCALL.byte() {
            return None;
        }
        if script[offset + 1..offset + 5] != Self::check_multisig_hash() {
            return None;
        }

        Some((m, public_keys))
    }

    /// Reads the `m`/`n` push value of a multi-sig script at `offset`.
    ///
    /// Accepts `PUSHINT8`, `PUSHINT16` (little-endian), and `PUSH1..PUSH16`, mirroring
    /// the C# opcode switch in `Helper.IsMultiSigContract`. Returns the value and the
    /// offset just past the push.
    fn read_multisig_push(script: &[u8], offset: usize) -> Option<(usize, usize)> {
        use neo_vm::OpCode;

        match script.get(offset).copied()? {
            b if b == OpCode::PUSHINT8.byte() => {
                let value = *script.get(offset + 1)? as usize;
                Some((value, offset + 2))
            }
            b if b == OpCode::PUSHINT16.byte() => {
                if script.len() < offset + 3 {
                    return None;
                }
                let value = u16::from_le_bytes([script[offset + 1], script[offset + 2]]) as usize;
                Some((value, offset + 3))
            }
            b if (OpCode::PUSH1.byte()..=OpCode::PUSH16.byte()).contains(&b) => {
                Some(((b - OpCode::PUSH0.byte()) as usize, offset + 1))
            }
            _ => None,
        }
    }

    /// Parses a multi-signature invocation script, returning the list of signatures when the
    /// script pushes the expected number of signatures encoded with `PUSHDATA1` opcodes.
    pub fn parse_multi_sig_invocation(
        invocation: &[u8],
        required_signatures: usize,
    ) -> Option<Vec<Vec<u8>>> {
        use neo_vm::OpCode;

        if required_signatures == 0 {
            return None;
        }

        let mut signatures = Vec::with_capacity(required_signatures);
        let mut offset = 0usize;
        while offset < invocation.len() {
            if invocation[offset] != OpCode::PUSHDATA1.byte() {
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

#[cfg(test)]
mod tests {
    use super::*;

    // Valid secp256r1 compressed public keys for testing
    fn test_pubkey() -> Vec<u8> {
        hex::decode("02486fd15702c4490a26703112a5cc1d0923fd697a33406bd5a1c00e0013b09a70")
            .expect("valid hex")
    }

    fn test_pubkey2() -> Vec<u8> {
        hex::decode("024c7b7fb6c310fccf1ba33b082519d82964ea93868d676662d4a59ad548df0e7d")
            .expect("valid hex")
    }

    fn test_pubkey3() -> Vec<u8> {
        hex::decode("02aaec38470f6aad0042c6e877cfd8087d2676b0f516fddd362801b9bd3936399e")
            .expect("valid hex")
    }

    #[test]
    fn signature_contract_cost_is_positive() {
        let cost = Helper::signature_contract_cost();
        assert!(cost > 0);
    }

    #[test]
    fn multi_signature_contract_cost_scales_with_m_and_n() {
        let cost_2_of_3 = Helper::multi_signature_contract_cost(2, 3);
        let cost_2_of_5 = Helper::multi_signature_contract_cost(2, 5);
        let cost_3_of_5 = Helper::multi_signature_contract_cost(3, 5);

        assert!(cost_2_of_5 > cost_2_of_3, "More keys should cost more");
        assert!(cost_3_of_5 > cost_2_of_5, "More signatures should cost more");
    }

    #[test]
    fn is_signature_contract_validates_correct_format() {
        let pubkey = test_pubkey();
        let script = Helper::signature_redeem_script(&pubkey);
        assert!(Helper::is_signature_contract(&script));
    }

    #[test]
    fn is_signature_contract_rejects_wrong_length() {
        let short_script = vec![0u8; 39];
        assert!(!Helper::is_signature_contract(&short_script));

        let long_script = vec![0u8; 41];
        assert!(!Helper::is_signature_contract(&long_script));
    }

    #[test]
    fn is_multi_sig_contract_accepts_valid_script() {
        let keys = vec![test_pubkey(), test_pubkey2(), test_pubkey3()];
        let script = Helper::multi_sig_redeem_script(2, &keys);
        assert!(Helper::is_multi_sig_contract(&script));
    }

    #[test]
    fn parse_multi_sig_contract_extracts_m_and_keys() {
        let keys = vec![test_pubkey(), test_pubkey2(), test_pubkey3()];
        let script = Helper::multi_sig_redeem_script(2, &keys);

        let result = Helper::parse_multi_sig_contract(&script);
        assert!(result.is_some());

        let (m, parsed_keys) = result.unwrap();
        assert_eq!(m, 2);
        assert_eq!(parsed_keys.len(), 3);
    }

    #[test]
    fn try_multi_sig_redeem_script_enforces_bounds() {
        let keys = vec![vec![0u8; 33]];

        // m = 0 is invalid
        assert!(Helper::try_multi_sig_redeem_script(0, &keys).is_err());

        // m > n is invalid
        assert!(Helper::try_multi_sig_redeem_script(2, &keys).is_err());
    }

    #[test]
    fn try_multi_sig_redeem_script_validates_key_format() {
        let invalid_key = vec![0xFFu8; 33];
        let keys = vec![invalid_key];
        assert!(Helper::try_multi_sig_redeem_script(1, &keys).is_err());
    }

    #[test]
    fn get_contract_hash_is_deterministic() {
        let sender = UInt160::from_bytes(&[1u8; 20]).expect("sender");
        let hash1 = Helper::get_contract_hash(&sender, 12345, "TestContract");
        let hash2 = Helper::get_contract_hash(&sender, 12345, "TestContract");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn parse_multi_sig_invocation_valid_signatures() {
        let sig1 = vec![0xAA; 64];
        let sig2 = vec![0xBB; 64];

        let mut invocation = Vec::new();
        invocation.push(OpCode::PUSHDATA1.byte());
        invocation.push(64);
        invocation.extend_from_slice(&sig1);
        invocation.push(OpCode::PUSHDATA1.byte());
        invocation.push(64);
        invocation.extend_from_slice(&sig2);

        let result = Helper::parse_multi_sig_invocation(&invocation, 2);
        assert!(result.is_some());

        let signatures = result.unwrap();
        assert_eq!(signatures.len(), 2);
        assert_eq!(signatures[0], sig1);
        assert_eq!(signatures[1], sig2);
    }

    #[test]
    fn parse_multi_sig_invocation_rejects_wrong_count() {
        let sig = vec![0xAA; 64];

        let mut invocation = Vec::new();
        invocation.push(OpCode::PUSHDATA1.byte());
        invocation.push(64);
        invocation.extend_from_slice(&sig);

        assert!(Helper::parse_multi_sig_invocation(&invocation, 2).is_none());
    }
}
