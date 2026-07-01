//! Neo N3 redeem-script primitives.
//!
//! Construction and pattern-recognition for the standard verification scripts
//! that back signature and multi-signature accounts — i.e. the byte sequences
//! whose `hash160` is the account script hash (= address). This is the Rust
//! counterpart of the redeem-script helpers in C# `Neo.SmartContract.Contract`
//! / `Neo.SmartContract.Helper`.
//!
//! This module is layered on `neo-crypto` (ECPoint), [`ScriptBuilder`] and
//! `neo-vm-rs` (OpCode / interop hashing), and sits *below* neo-core so the
//! chain types (Witness/Signer) and wallet layers can build and recognize
//! verification scripts without depending on the smart-contract engine.
//!
//! These bytes are consensus-critical: they determine script hashes and
//! therefore addresses, so the encoding must stay byte-identical to C# Neo
//! v3.9.1 (including the ascending public-key sort in multi-sig scripts, which
//! matches C# `ECPoint.CompareTo`).

use neo_crypto::{ECCurve, ECPoint};
use neo_primitives::UInt160;
use neo_vm_rs::OpCode;

use super::ScriptBuilder;

/// Errors raised while constructing a redeem script.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RedeemScriptError {
    /// The requested redeem-script construction is invalid (bad m/n, bad key).
    #[error("{0}")]
    InvalidOperation(String),
}

impl RedeemScriptError {
    /// Creates an [`RedeemScriptError::InvalidOperation`] from any message.
    pub fn invalid_operation(message: impl Into<String>) -> Self {
        Self::InvalidOperation(message.into())
    }
}

impl From<RedeemScriptError> for neo_error::CoreError {
    fn from(err: RedeemScriptError) -> Self {
        neo_error::CoreError::InvalidOperation {
            message: err.to_string(),
        }
    }
}

/// Neo N3 redeem-script primitives (construction and pattern recognition for
/// the standard signature / multi-signature verification scripts).
pub struct RedeemScript;

impl RedeemScript {
    /// The dBFT agreement threshold `M = N - (N - 1) / 3` for `n` consensus
    /// nodes (C# `Contract.GetBFTAddress` / `(Validators.Length - (n - 1) / 3)`).
    /// Returns 0 for `n == 0`. The single source of truth for this
    /// consensus-critical formula across the workspace.
    #[must_use]
    pub fn bft_threshold(n: usize) -> usize {
        if n == 0 { 0 } else { n - (n - 1) / 3 }
    }

    /// C# `Contract.GetBFTAddress(pubkeys)`: the script hash of the
    /// `bft_threshold(N)`-of-`N` multi-signature contract over `pubkeys`
    /// (validators or oracle nodes). Returns `None` for an empty set or when the
    /// multi-sig script cannot be built.
    #[must_use]
    pub fn bft_address(pubkeys: &[ECPoint]) -> Option<UInt160> {
        if pubkeys.is_empty() {
            return None;
        }
        let m = Self::bft_threshold(pubkeys.len());
        let script = Self::multi_sig_redeem_script_from_points(m, pubkeys).ok()?;
        Some(UInt160::from_script(&script))
    }

    /// Creates a signature redeem script from a raw (compressed) public key.
    ///
    /// Layout (40 bytes): `PUSHDATA1 0x21 <33-byte pubkey> SYSCALL <CheckSig hash>`.
    pub fn signature_redeem_script(public_key: &[u8]) -> Vec<u8> {
        let mut script = Vec::new();
        script.push(OpCode::PUSHDATA1.byte());
        script.push(public_key.len() as u8);
        script.extend_from_slice(public_key);
        script.push(OpCode::SYSCALL.byte());
        script.extend_from_slice(&Self::check_sig_hash());
        script
    }

    /// Checks whether `script` is a single-signature verification script.
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

    /// Checks whether `script` is a multi-signature verification script (C#
    /// `Helper.IsMultiSigContract`). Delegates to [`Self::parse_multi_sig_contract`] so
    /// the same `PUSHINT8`/`PUSHINT16`/`PUSH1..16` `m`/`n` decode and `1 <= m <= n
    /// <= 1024` bounds apply (committee-sized multisigs are recognized).
    pub fn is_multi_sig_contract(script: &[u8]) -> bool {
        Self::parse_multi_sig_contract(script).is_some()
    }

    /// Creates a multi-sig redeem script from already-parsed public-key points.
    ///
    /// Mirrors C# `Contract.CreateMultiSigRedeemScript`: emits `PUSH(m)`, each
    /// public key in ascending order, `PUSH(n)`, then `SYSCALL CheckMultisig`.
    ///
    /// # Errors
    ///
    /// Returns [`RedeemScriptError`] if `public_keys` is empty, exceeds 1024, or
    /// `m` is not in range `1..=n`.
    pub fn multi_sig_redeem_script_from_points(
        m: usize,
        public_keys: &[ECPoint],
    ) -> Result<Vec<u8>, RedeemScriptError> {
        let n = public_keys.len();
        if n == 0 {
            return Err(RedeemScriptError::invalid_operation(
                "No public keys provided for multi-sig contract",
            ));
        }
        if n > 1024 {
            return Err(RedeemScriptError::invalid_operation(format!(
                "Too many public keys: {} (max 1024)",
                n
            )));
        }
        if !(1..=n).contains(&m) {
            return Err(RedeemScriptError::invalid_operation(format!(
                "Invalid multi-sig parameters: m={}, n={}",
                m, n
            )));
        }

        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(m as i64);

        let mut sorted_keys = public_keys.to_vec();
        sorted_keys.sort();
        for key in sorted_keys.iter() {
            let encoded = key.encode_point(true).unwrap_or_else(|_| key.to_bytes());
            builder.emit_push(&encoded);
        }

        builder.emit_push_int(n as i64);
        builder
            .emit_syscall("System.Crypto.CheckMultisig")
            .map_err(|err| {
                RedeemScriptError::invalid_operation(format!(
                    "Failed to build contract script: {}",
                    err
                ))
            })?;

        Ok(builder.to_array())
    }

    /// Creates a multi-sig redeem script from raw (compressed) public-key bytes.
    ///
    /// Raw-bytes wrapper: parses each key to an [`ECPoint`], then delegates to
    /// [`Self::multi_sig_redeem_script_from_points`], which enforces C#
    /// `Contract.CreateMultiSigRedeemScript`'s bounds (`1 <= m <= n <= 1024`).
    ///
    /// # Errors
    ///
    /// Returns [`RedeemScriptError`] if `m`/`n` are out of range (`1 <= m <= n <=
    /// 1024`) or any key fails to parse.
    pub fn multi_sig_redeem_script_from_keys(
        m: usize,
        public_keys: &[Vec<u8>],
    ) -> Result<Vec<u8>, RedeemScriptError> {
        let mut points = Vec::with_capacity(public_keys.len());
        for key in public_keys {
            let point = ECPoint::from_bytes_with_curve(ECCurve::Secp256r1, key).map_err(|e| {
                RedeemScriptError::invalid_operation(format!("Invalid public key: {e}"))
            })?;
            points.push(point);
        }

        Self::multi_sig_redeem_script_from_points(m, &points)
    }

    /// Parses a multi-signature verification script, returning `(m, ordered public
    /// keys)` when the script matches the canonical Neo multi-sig format. The
    /// inverse of [`Self::multi_sig_redeem_script_from_keys`] /
    /// [`Self::is_multi_sig_contract`].
    ///
    /// Mirrors C# `Helper.IsMultiSigContract`: `m` and `n` are decoded as integer
    /// pushes (`PUSHINT8`/`PUSHINT16`/`PUSH1..16`) and bounded by `1 <= m <= n <=
    /// 1024`, so committee-sized multisigs (e.g. 21 keys, encoded via `PUSHINT8`)
    /// are recognized — not just `n <= 16`.
    pub fn parse_multi_sig_contract(script: &[u8]) -> Option<(usize, Vec<Vec<u8>>)> {
        if script.len() < 42 {
            return None;
        }

        let (m, m_size) = read_multisig_count(script, 0)?;
        if !(1..=1024).contains(&m) {
            return None;
        }
        let mut offset = m_size;

        let mut public_keys = Vec::new();
        while script.get(offset) == Some(&OpCode::PUSHDATA1.byte()) {
            // PUSHDATA1 (1) + length byte (1) + 33-byte key = 35 bytes; require at
            // least one trailing byte for the `n` push (C#: len <= i + 35).
            if script.len() <= offset + 35 || script[offset + 1] != 33 {
                return None;
            }
            let public_key = &script[offset + 2..offset + 35];
            if ECPoint::from_bytes(public_key).is_err() {
                return None;
            }
            public_keys.push(public_key.to_vec());
            offset += 35;
        }

        let n = public_keys.len();
        if n < m || n > 1024 {
            return None;
        }

        let (n_decoded, n_size) = read_multisig_count(script, offset)?;
        if n_decoded != n {
            return None;
        }
        offset += n_size;

        if script.len() != offset + 5
            || script[offset] != OpCode::SYSCALL.byte()
            || script[offset + 1..offset + 5] != Self::check_multisig_hash()
        {
            return None;
        }

        Some((m, public_keys))
    }

    /// Parses a multi-signature invocation script, returning the signatures when it
    /// pushes exactly `required_signatures` 64-byte signatures via `PUSHDATA1`.
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

    /// Gets the `System.Crypto.CheckSig` syscall hash (little-endian) — the 4-byte
    /// suffix of a single-signature verification script.
    pub fn check_sig_hash() -> [u8; 4] {
        syscall_hash("System.Crypto.CheckSig")
    }

    /// Gets the `System.Crypto.CheckMultisig` syscall hash (little-endian) — the
    /// 4-byte suffix of a multi-signature verification script.
    pub fn check_multisig_hash() -> [u8; 4] {
        syscall_hash("System.Crypto.CheckMultisig")
    }
}

/// Decodes an `m`/`n` count from a multi-sig script at `offset`, mirroring C#
/// `Helper.IsMultiSigContract`: the value may be a `PUSHINT8`, `PUSHINT16`, or a
/// `PUSH1..PUSH16` opcode. Returns `(value, bytes_consumed)`.
fn read_multisig_count(script: &[u8], offset: usize) -> Option<(usize, usize)> {
    let op = *script.get(offset)?;
    if op == OpCode::PUSHINT8.byte() {
        Some((*script.get(offset + 1)? as usize, 2))
    } else if op == OpCode::PUSHINT16.byte() {
        let bytes = script.get(offset + 1..offset + 3)?;
        Some((u16::from_le_bytes([bytes[0], bytes[1]]) as usize, 3))
    } else if (OpCode::PUSH1.byte()..=OpCode::PUSH16.byte()).contains(&op) {
        Some(((op - OpCode::PUSH0.byte()) as usize, 1))
    } else {
        None
    }
}

/// Computes a syscall hash via neo-vm-rs interop hashing.
fn syscall_hash(name: &str) -> [u8; 4] {
    neo_vm_rs::interop_hash(name).to_le_bytes()
}

#[cfg(test)]
#[path = "../tests/script_builder/redeem_script.rs"]
mod tests;
