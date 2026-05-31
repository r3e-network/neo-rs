//! Neo N3 redeem-script primitives.
//!
//! Construction and pattern-recognition for the standard verification scripts
//! that back signature and multi-signature accounts — i.e. the byte sequences
//! whose `hash160` is the account script hash (= address). This is the Rust
//! counterpart of the redeem-script helpers in C# `Neo.SmartContract.Contract`
//! / `Neo.SmartContract.Helper`.
//!
//! The crate is layered on `neo-crypto` (ECPoint), `neo-script-builder`
//! (ScriptBuilder) and `neo-vm-rs` (OpCode / interop hashing), and sits *below*
//! neo-core so the chain types (Witness/Signer) and wallet layers can build and
//! recognize verification scripts without depending on the smart-contract
//! engine.
//!
//! These bytes are consensus-critical: they determine script hashes and
//! therefore addresses, so the encoding must stay byte-identical to C# Neo
//! v3.9.1 (including the ascending public-key sort in multi-sig scripts, which
//! matches C# `ECPoint.CompareTo`).

use neo_crypto::ECPoint;
use neo_script_builder::ScriptBuilder;
use neo_vm_rs::OpCode;

/// Errors raised while constructing a redeem script.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RedeemScriptError {
    /// The requested redeem-script construction is invalid (bad m/n, bad key).
    #[error("{0}")]
    InvalidOperation(String),
}

impl RedeemScriptError {
    /// Creates a [`RedeemScriptError::InvalidOperation`] from any message.
    pub fn invalid_operation(message: impl Into<String>) -> Self {
        Self::InvalidOperation(message.into())
    }
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
    script.extend_from_slice(&check_sig_hash());
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
    script[36..40] == check_sig_hash()
}

/// Checks whether `script` is a multi-signature verification script.
pub fn is_multi_sig_contract(script: &[u8]) -> bool {
    if script.len() < 42 {
        return false;
    }

    // Check basic pattern for multi-sig
    let _m = match script[0] {
        value if (OpCode::PUSH1.byte()..=OpCode::PUSH16.byte()).contains(&value) => {
            value - OpCode::PUSH0.byte()
        }
        _ => return false,
    };

    // Verify ending with SYSCALL CheckMultisig
    let len = script.len();
    script[len - 5] == OpCode::SYSCALL.byte()
        && script[len - 4..] == check_multisig_hash()
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
    builder.emit_syscall("System.Crypto.CheckMultisig").map_err(|err| {
        RedeemScriptError::invalid_operation(format!("Failed to build contract script: {}", err))
    })?;

    Ok(builder.to_array())
}

/// Creates a multi-sig redeem script from raw (compressed) public-key bytes.
///
/// Standard-account wrapper: limits `m` and `n` to `1..=16`, parses each key to
/// an [`ECPoint`], then delegates to [`multi_sig_redeem_script_from_points`].
///
/// # Errors
///
/// Returns [`RedeemScriptError`] if `m` is not in range `1..=16`, more than 16
/// keys are supplied, `m` exceeds the key count, or any key fails to parse.
pub fn multi_sig_redeem_script_from_keys(
    m: usize,
    public_keys: &[Vec<u8>],
) -> Result<Vec<u8>, RedeemScriptError> {
    if !(1..=16).contains(&m) || public_keys.len() > 16 || m > public_keys.len() {
        return Err(RedeemScriptError::invalid_operation(format!(
            "Invalid multi-sig parameters: m={}, n={}",
            m,
            public_keys.len()
        )));
    }

    let mut points = Vec::with_capacity(public_keys.len());
    for key in public_keys {
        let point = ECPoint::from_bytes(key)
            .map_err(|e| RedeemScriptError::invalid_operation(format!("Invalid public key: {e}")))?;
        points.push(point);
    }

    multi_sig_redeem_script_from_points(m, &points)
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

/// Computes a syscall hash via neo-vm-rs interop hashing.
fn syscall_hash(name: &str) -> [u8; 4] {
    neo_vm_rs::interop_hash(name).to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    // 33-byte compressed secp256r1 public keys (valid points) used as fixtures.
    fn key_a() -> Vec<u8> {
        hex_to_bytes("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
    }
    fn key_b() -> Vec<u8> {
        hex_to_bytes("02103a7f7dd016558597f7960d27c516a4394fd968b9e65155eb4b013e4040406e")
    }

    fn hex_to_bytes(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn signature_script_has_csharp_layout() {
        let pk = key_a();
        let script = signature_redeem_script(&pk);
        assert_eq!(script.len(), 40);
        assert_eq!(script[0], OpCode::PUSHDATA1.byte());
        assert_eq!(script[1], 33);
        assert_eq!(&script[2..35], &pk[..]);
        assert_eq!(script[35], OpCode::SYSCALL.byte());
        assert_eq!(&script[36..40], &syscall_hash("System.Crypto.CheckSig"));
        assert!(is_signature_contract(&script));
        assert!(!is_multi_sig_contract(&script));
    }

    #[test]
    fn multisig_script_from_keys_matches_points() {
        let keys = vec![key_a(), key_b()];
        let from_keys = multi_sig_redeem_script_from_keys(2, &keys).expect("from keys");

        let points: Vec<ECPoint> = keys.iter().map(|k| ECPoint::from_bytes(k).unwrap()).collect();
        let from_points = multi_sig_redeem_script_from_points(2, &points).expect("from points");

        assert_eq!(from_keys, from_points);
        assert!(is_multi_sig_contract(&from_keys));
        assert!(!is_signature_contract(&from_keys));
        // C# 2-of-2 layout: PUSH2 .. PUSH2 SYSCALL CheckMultisig
        assert_eq!(from_keys[0], OpCode::PUSH2.byte());
        assert_eq!(from_keys[from_keys.len() - 5], OpCode::SYSCALL.byte());
        assert_eq!(
            &from_keys[from_keys.len() - 4..],
            &syscall_hash("System.Crypto.CheckMultisig")
        );
    }

    #[test]
    fn multisig_key_order_is_canonical() {
        // Output must be independent of input order (keys are sorted ascending).
        let forward = multi_sig_redeem_script_from_keys(2, &[key_a(), key_b()]).unwrap();
        let reverse = multi_sig_redeem_script_from_keys(2, &[key_b(), key_a()]).unwrap();
        assert_eq!(forward, reverse);
    }

    #[test]
    fn multisig_rejects_invalid_params() {
        assert!(multi_sig_redeem_script_from_keys(0, &[key_a()]).is_err());
        assert!(multi_sig_redeem_script_from_keys(3, &[key_a(), key_b()]).is_err());
        assert!(multi_sig_redeem_script_from_points(1, &[]).is_err());
    }
}
