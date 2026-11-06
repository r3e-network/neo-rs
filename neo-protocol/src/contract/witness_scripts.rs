// Copyright (c) 2024 R3E Network
// Licensed under the MIT License. See LICENSE file for details.

//! Pure witness-script construction and classification helpers.
//!
//! Migrated from the witness/contract portion of `neo-core::smart_contract::helper`.
//! Only the self-contained, VM-level helpers are included here: redeem-script
//! builders, standard/signature/multi-sig classification, multi-sig script and
//! invocation parsing, and deployed-contract hash derivation. The
//! execution-coupled helpers (`verify_witnesses`, verification gas costing, and
//! anything touching `ApplicationEngine`/`DataCache`) intentionally remain in the
//! higher layers and are **not** part of this migration.
//!
//! `CoreError`/`CoreResult` are replaced by the local [`ContractError`], keeping
//! dependencies within `neo-crypto`, `neo-primitives`, and `neo-vm`.

use crate::builders::ScriptBuilder;
use crate::contract::helpers::ContractError;
use neo_crypto::{ECCurve, ECPoint};
use neo_primitives::UInt160;
use neo_vm::OpCode;

/// Witness-script builders and classifiers (matches the pure portion of C# `Helper`).
pub struct WitnessScript;

impl WitnessScript {
    /// Maximum number of public keys allowed in a multi-sig contract (C# neo v3.10.2).
    pub const MAX_PUBLIC_KEYS_PER_MULTISIG: usize = 1024;

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
    /// Returns [`ContractError`] if:
    /// - `m` is not in range `1..=1024`
    /// - `public_keys.len()` exceeds 1024
    /// - `m` exceeds `public_keys.len()`
    /// - any key is not a valid secp256r1 point
    pub fn try_multi_sig_redeem_script(
        m: usize,
        public_keys: &[Vec<u8>],
    ) -> Result<Vec<u8>, ContractError> {
        let max = Self::MAX_PUBLIC_KEYS_PER_MULTISIG;
        if !(1..=max).contains(&m) || public_keys.len() > max || m > public_keys.len() {
            return Err(ContractError::invalid_operation(format!(
                "Invalid multi-sig parameters: m={}, n={}",
                m,
                public_keys.len()
            )));
        }

        let mut points = Vec::with_capacity(public_keys.len());
        for key in public_keys {
            let point = ECPoint::from_bytes(key).map_err(|e| {
                ContractError::invalid_operation(format!("Invalid public key: {e}"))
            })?;
            points.push(point);
        }

        crate::contract::helpers::Contract::try_create_multi_sig_redeem_script(m, &points)
    }

    /// Creates a multi-sig redeem script (panics on invalid input).
    ///
    /// Prefer `try_multi_sig_redeem_script` for fallible construction.
    #[inline]
    pub fn multi_sig_redeem_script(m: usize, public_keys: &[Vec<u8>]) -> Vec<u8> {
        Self::try_multi_sig_redeem_script(m, public_keys).expect("Invalid multi-sig parameters")
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

    /// Checks if a script is a multi-sig contract.
    ///
    /// This is the only classification path. C# v3.10.2 decodes every public key
    /// before any `IsMultiSigContract` overload returns true; a shape-only check
    /// let a malformed key through the `bool` overload.
    pub fn is_multi_sig_contract(script: &[u8]) -> bool {
        Self::parse_multi_sig_contract(script).is_some()
    }

    /// Parses a multi-signature contract script, returning the required signature count and
    /// the ordered public keys when the script matches the canonical Neo multi-sig format.
    ///
    /// Mirrors C# `Helper.IsMultiSigContract` (v3.10.2): m/n accept `PUSHINT8`/`PUSHINT16`
    /// as well as `PUSH1..PUSH16`, both are bounded by 1..=1024, every 33-byte key must
    /// decode as a secp256r1 point, and the tail must be `n`-push, `SYSCALL`, `CheckMultisig`.
    /// Decoding runs even when the caller only needs a boolean.
    pub fn parse_multi_sig_contract(script: &[u8]) -> Option<(usize, Vec<Vec<u8>>)> {
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
            ECPoint::from_bytes_with_curve(ECCurve::Secp256r1, key).ok()?;
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
}
