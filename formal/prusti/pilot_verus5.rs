//! Verus pilot 5 — Phase B remainder: protocol enums + consensus validation.
//!
//! Verus counterpart of the Prusti `pilot_batch6.rs`. Verus supports Option
//! and match natively, so these mirrors are structurally identical to the
//! real sources:
//!   - TransactionAttributeType: bytes {0x01,0x11,0x20,0x21,0x22}, only
//!     Conflicts (0x21) allows multiple,
//!   - ContractParameterType: 13 byte codes, 0x21 unassigned (wire trap),
//!   - ChangeView::new_view_number = view+1 with u8 overflow rejection,
//!     validate demands strict increase (always true without overflow),
//!   - CommitMessage::validate: ECDSA signature exactly 64 bytes,
//!   - uint macro from_bytes: slice length must equal the fixed size.
//!
//! Re-run:  cd .tools && ./verus.exe ../formal/prusti/pilot_verus5.rs

use vstd::prelude::*;

verus! {

// ---- TransactionAttributeType ----------------------------------------------

pub open spec fn attr_from_byte(value: u8) -> Option<u8> {
    match value {
        0x01 => Some(0x01u8),   // HighPriority
        0x11 => Some(0x11u8),   // OracleResponse
        0x20 => Some(0x20u8),   // NotValidBefore
        0x21 => Some(0x21u8),   // Conflicts
        0x22 => Some(0x22u8),   // NotaryAssisted
        _    => None,
    }
}

pub open spec fn attr_allows_multiple(value: u8) -> bool {
    value == 0x21
}

proof fn attr_unknown_rejected()
    ensures attr_from_byte(0xFFu8) == None,
{}

proof fn attr_conflicts_multiple()
    ensures attr_allows_multiple(0x21u8),
{}

/// Only Conflicts allows multiple instances (parametric over known bytes).
proof fn attr_multiple_only_conflicts(b: u8)
    requires b != 0x21 && attr_from_byte(b).is_some(),
    ensures !attr_allows_multiple(b),
{}

/// Roundtrip: every accepted byte is a fixed point of from_byte.
proof fn attr_roundtrip(b: u8)
    requires attr_from_byte(b).is_some(),
    ensures attr_from_byte(b) == Some(b),
{}

// ---- ContractParameterType ---------------------------------------------------

pub open spec fn cpt_from_byte(value: u8) -> Option<u8> {
    match value {
        0x00 => Some(0x00u8),   // Any
        0x10 => Some(0x10u8),   // Boolean
        0x11 => Some(0x11u8),   // Integer
        0x12 => Some(0x12u8),   // ByteArray
        0x13 => Some(0x13u8),   // String
        0x14 => Some(0x14u8),   // Hash160
        0x15 => Some(0x15u8),   // Hash256
        0x16 => Some(0x16u8),   // PublicKey
        0x17 => Some(0x17u8),   // Signature
        0x20 => Some(0x20u8),   // Array
        0x22 => Some(0x22u8),   // Map
        0x30 => Some(0x30u8),   // InteropInterface
        0xFF => Some(0xFFu8),   // Void
        _    => None,
    }
}

proof fn cpt_void_known()
    ensures cpt_from_byte(0xFFu8) == Some(0xFFu8),
{}

/// Wire trap: 0x21 is a valid attribute byte but not a parameter type.
proof fn cpt_0x21_unassigned()
    ensures cpt_from_byte(0x21u8) == None,
{}

proof fn cpt_roundtrip(b: u8)
    requires cpt_from_byte(b).is_some(),
    ensures cpt_from_byte(b) == Some(b),
{}

// ---- ChangeView --------------------------------------------------------------

pub open spec fn cv_new_view_spec(view: u8) -> Option<u8> {
    if view == 255 { None } else { Some((view + 1) as u8) }
}

pub exec fn cv_new_view(view: u8) -> (r: Option<u8>)
    ensures r == cv_new_view_spec(view),
{
    if view == 255 { None } else { Some(view + 1u8) }
}

pub open spec fn cv_validate_ok(view: u8) -> bool {
    cv_new_view_spec(view).is_some()
}

/// Without overflow the new view is strictly larger, so the strict-increase
/// check in `ChangeView::validate` always passes.
proof fn cv_strict_increase(view: u8)
    requires view < 255,
    ensures
        cv_new_view_spec(view) == Some((view + 1) as u8),
        view + 1 > view,
{}

proof fn cv_overflow_rejected()
    ensures !cv_validate_ok(255u8),
{}

proof fn cv_zero_ok()
    ensures cv_validate_ok(0u8),
{}

// ---- Commit ------------------------------------------------------------------

pub open spec fn commit_sig_ok(len: usize) -> bool {
    len == 64
}

proof fn commit_short_rejected()
    ensures !commit_sig_ok(63),
{}

proof fn commit_long_rejected()
    ensures !commit_sig_ok(65),
{}

proof fn commit_ok()
    ensures commit_sig_ok(64),
{}

// ---- UInt from_bytes ---------------------------------------------------------

pub open spec fn uint_from_bytes_ok(len: usize, size: usize) -> bool {
    len == size
}

proof fn uint160_ok()
    ensures uint_from_bytes_ok(20, 20),
{}

proof fn uint160_short_rejected()
    ensures !uint_from_bytes_ok(19, 20),
{}

proof fn uint256_ok()
    ensures uint_from_bytes_ok(32, 32),
{}

proof fn uint256_long_rejected()
    ensures !uint_from_bytes_ok(33, 32),
{}

fn main() {}
}
