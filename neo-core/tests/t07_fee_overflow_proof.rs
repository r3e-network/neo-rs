//! T07 security audit — empirical proof for the combined-fee overflow check.
//!
//! ADDITIVE ONLY (new integration test file); no `src/` file is modified.
//!
//! `Transaction::deserialize_unsigned` (neo-core/src/network/p2p/payloads/
//! transaction/serialization.rs:48) validates the combined fee with
//! `if system_fee + network_fee < system_fee { ... }` AFTER separately checking
//! that both values are >= 0. With two `i64::MAX` fees the addition overflows:
//!   * builds with `overflow-checks = true` (Cargo.toml `[profile.dev]`, which
//!     `[profile.test]` inherits) => **panic**;
//!   * release builds (`[profile.release]`, no `overflow-checks`) wrap silently,
//!     and the wrapped value happens to be < system_fee so the error is returned.
//! The two profiles therefore behave differently for identical attacker-supplied
//! bytes, and a debug/`overflow-checks` build can be crashed remotely.

use neo_core::neo_io::{MemoryReader, Serializable};
use neo_core::network::p2p::payloads::Transaction;

/// Build a minimal but structurally valid transaction wire payload with the
/// supplied system/network fees.
fn tx_bytes(system_fee: i64, network_fee: i64) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0u8); // version
    out.extend_from_slice(&0u32.to_le_bytes()); // nonce
    out.extend_from_slice(&system_fee.to_le_bytes()); // system_fee  (i64 LE)
    out.extend_from_slice(&network_fee.to_le_bytes()); // network_fee (i64 LE)
    out.extend_from_slice(&0u32.to_le_bytes()); // valid_until_block

    // signers: 1 x Signer{ account: 20B, scopes: 0x00 (None) }
    out.push(1u8); // varint count = 1
    out.extend_from_slice(&[0x11u8; 20]); // UInt160
    out.push(0x00u8); // WitnessScope::None

    out.push(0u8); // attributes: count = 0

    // script: varbytes len=1, [0x00]
    out.push(1u8);
    out.push(0x00u8);

    // witnesses: 1 x Witness{ invocation: varbytes(0), verification: varbytes(0) }
    out.push(1u8);
    out.push(0u8);
    out.push(0u8);

    out
}

#[test]
fn combined_fee_overflow_behaviour_is_profile_dependent() {
    let bytes = tx_bytes(i64::MAX, i64::MAX);

    let outcome = std::panic::catch_unwind(move || {
        let mut reader = MemoryReader::new(&bytes);
        Transaction::deserialize(&mut reader)
    });

    match outcome {
        Err(panic_payload) => {
            let message = panic_payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic_payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_else(|| "<non-string panic payload>".to_string());
            println!("T07-EVIDENCE: PANIC on i64::MAX fees: {message}");
            // With overflow-checks enabled the addition panics instead of
            // returning IoError. That is the debug/release divergence.
            assert!(
                message.contains("overflow"),
                "unexpected panic message: {message}"
            );
        }
        Ok(result) => {
            println!("T07-EVIDENCE: no panic; result = {result:?}");
            // Release behaviour: wrapping makes the guard fire and reject.
            assert!(result.is_err(), "overflowing fees must never be accepted");
        }
    }

    // Sanity: a well-formed small fee pair must still deserialize.
    let ok_bytes = tx_bytes(1, 1);
    let mut reader = MemoryReader::new(&ok_bytes);
    let tx = Transaction::deserialize(&mut reader).expect("valid tx");
    assert_eq!(tx.system_fee(), 1);
    assert_eq!(tx.network_fee(), 1);
}
