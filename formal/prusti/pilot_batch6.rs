//! Prusti pilot batch 6 — Phase B remainder: protocol enums, consensus
//! message validation, UInt length checks.
//!
//! Faithful mirrors of:
//!   - neo-primitives/src/transaction_attribute_type.rs (wire bytes 0x01,
//!     0x11, 0x20, 0x21, 0x22; only Conflicts allows multiple instances),
//!   - neo-primitives/src/contract_parameter_type.rs (13 byte codes incl.
//!     Void = 0xFF; note 0x21 is NOT a parameter type — Array=0x20/Map=0x22),
//!   - neo-consensus/src/messages/change_view.rs (`new_view_number` =
//!     view+1 with u8 overflow rejection; validate demands strict increase),
//!   - neo-consensus/src/messages/commit.rs (ECDSA signature must be 64 bytes),
//!   - neo-primitives/src/macros.rs uint `from_bytes` (length must equal N).
//!
//! Prusti 0.2.2 constraints honored: if/else only (no match, no Shl), no
//! `const` items referenced inside pure fns, literals everywhere, and NO
//! `Option` values in pure code (is_some/is_none are "impure" and Option
//! constants unsupported) — mirrors use the u16 sentinel 0x0100 for "None",
//! which no valid u8 byte can equal.

use prusti_contracts::*;

// ---- TransactionAttributeType (mirror over the wire byte) -------------------

/// Mirror of `TransactionAttributeType::from_byte`: the known byte itself,
/// or the sentinel for unknown bytes.
#[pure]
#[ensures(result == 0x0100 || result == value as u16)]
#[ensures(result != 0)]
pub fn attr_from_byte(value: u8) -> u16 {
    if value == 0x01 {
        0x01
    } else if value == 0x11 {
        0x11
    } else if value == 0x20 {
        0x20
    } else if value == 0x21 {
        0x21
    } else if value == 0x22 {
        0x22
    } else {
        0x0100
    }
}

/// Mirror of `allows_multiple`: only Conflicts (0x21).
#[pure]
#[ensures(result == (value == 0x21))]
pub fn attr_allows_multiple(value: u8) -> bool {
    value == 0x21
}

#[pure]
#[ensures(attr_from_byte(0xFF) == 0x0100)]
pub fn attr_unknown_rejected() -> bool {
    true
}

#[pure]
#[ensures(attr_from_byte(0x01) == 0x01 && !attr_allows_multiple(0x01))]
pub fn attr_high_priority_single() -> bool {
    true
}

#[pure]
#[ensures(attr_from_byte(0x11) == 0x11 && !attr_allows_multiple(0x11))]
pub fn attr_oracle_single() -> bool {
    true
}

#[pure]
#[ensures(attr_from_byte(0x20) == 0x20 && !attr_allows_multiple(0x20))]
pub fn attr_nvb_single() -> bool {
    true
}

#[pure]
#[ensures(attr_from_byte(0x21) == 0x21 && attr_allows_multiple(0x21))]
pub fn attr_conflicts_multiple() -> bool {
    true
}

#[pure]
#[ensures(attr_from_byte(0x22) == 0x22 && !attr_allows_multiple(0x22))]
pub fn attr_notary_single() -> bool {
    true
}

/// Roundtrip: every accepted byte is a fixed point of from_byte.
#[pure]
#[requires(attr_from_byte(value) != 0x0100)]
#[ensures(attr_from_byte(value) == value as u16)]
pub fn attr_roundtrip(value: u8) -> bool {
    true
}

// ---- ContractParameterType (mirror over the wire byte) ----------------------

/// Mirror of `ContractParameterType::try_from_u8`/`from_byte`.
#[pure]
#[ensures(result == 0x0100 || result == value as u16)]
pub fn cpt_from_byte(value: u8) -> u16 {
    if value == 0x00 {
        0x00
    } else if value == 0x10 {
        0x10
    } else if value == 0x11 {
        0x11
    } else if value == 0x12 {
        0x12
    } else if value == 0x13 {
        0x13
    } else if value == 0x14 {
        0x14
    } else if value == 0x15 {
        0x15
    } else if value == 0x16 {
        0x16
    } else if value == 0x17 {
        0x17
    } else if value == 0x20 {
        0x20
    } else if value == 0x22 {
        0x22
    } else if value == 0x30 {
        0x30
    } else if value == 0xFF {
        0xFF
    } else {
        0x0100
    }
}

#[pure]
#[ensures(cpt_from_byte(0x11) == 0x11)]
pub fn cpt_integer_known() -> bool {
    true
}

#[pure]
#[ensures(cpt_from_byte(0xFF) == 0xFF)]
pub fn cpt_void_known() -> bool {
    true
}

/// Wire trap: 0x21 is a valid attribute byte (Conflicts) but NOT a valid
/// parameter type (Array=0x20, Map=0x22 — 0x21 is unassigned).
#[pure]
#[ensures(cpt_from_byte(0x21) == 0x0100)]
pub fn cpt_0x21_unassigned() -> bool {
    true
}

#[pure]
#[ensures(cpt_from_byte(0x02) == 0x0100)]
pub fn cpt_unknown_rejected() -> bool {
    true
}

#[pure]
#[requires(cpt_from_byte(value) != 0x0100)]
#[ensures(cpt_from_byte(value) == value as u16)]
pub fn cpt_roundtrip(value: u8) -> bool {
    true
}

// ---- ChangeView new view number / validate (mirror) -------------------------

/// Mirror of `ChangeView::new_view_number`: view+1, sentinel on u8 overflow.
#[pure]
#[ensures(result == 0x0100 || result == view as u16 + 1)]
pub fn cv_new_view(view: u8) -> u16 {
    if view == 255 {
        0x0100
    } else {
        view as u16 + 1
    }
}

/// Mirror of `ChangeView::validate` outcome: 0 = Ok, 1 = overflow.
/// (The "not strictly larger" branch is unreachable: view+1 > view whenever
/// the addition does not overflow — proven by `cv_strict_increase`.)
#[pure]
#[ensures(result == 0 || result == 1)]
#[ensures(result == 1 ==> view == 255)]
pub fn cv_validate(view: u8) -> u8 {
    if cv_new_view(view) == 0x0100 {
        1
    } else {
        0
    }
}

/// Without overflow the new view number is strictly larger than the old one,
/// so `ChangeView::validate`'s strict-increase check always passes.
#[pure]
#[requires(view < 255)]
#[ensures(cv_new_view(view) == view as u16 + 1)]
#[ensures(view + 1 > view)]
pub fn cv_strict_increase(view: u8) -> bool {
    true
}

#[pure]
#[ensures(cv_validate(255) == 1)]
pub fn cv_overflow_rejected() -> u8 {
    cv_validate(255)
}

#[pure]
#[ensures(cv_validate(0) == 0)]
pub fn cv_zero_ok() -> u8 {
    cv_validate(0)
}

// ---- Commit signature length (mirror) ---------------------------------------

/// Mirror of `CommitMessage::validate`: ECDSA signature is exactly 64 bytes.
#[pure]
#[ensures(result == 0 ==> sig_len == 64)]
#[ensures(result == 1 ==> sig_len != 64)]
pub fn commit_validate(sig_len: usize) -> u8 {
    if sig_len == 64 {
        0
    } else {
        1
    }
}

#[pure]
#[ensures(commit_validate(63) == 1)]
pub fn commit_short_rejected() -> u8 {
    commit_validate(63)
}

#[pure]
#[ensures(commit_validate(65) == 1)]
pub fn commit_long_rejected() -> u8 {
    commit_validate(65)
}

// ---- UInt from_bytes length check (mirror) ----------------------------------

/// Mirror of the uint-macro `from_bytes` guard: length must equal the fixed
/// size (20 for UInt160, 32 for UInt256).
#[pure]
#[ensures(result == (len == size))]
pub fn uint_from_bytes_ok(len: usize, size: usize) -> bool {
    len == size
}

#[pure]
#[ensures(uint_from_bytes_ok(20, 20))]
pub fn uint160_ok() -> bool {
    true
}

#[pure]
#[ensures(!uint_from_bytes_ok(19, 20))]
pub fn uint160_short_rejected() -> bool {
    true
}

#[pure]
#[ensures(uint_from_bytes_ok(32, 32))]
pub fn uint256_ok() -> bool {
    true
}

#[pure]
#[ensures(!uint_from_bytes_ok(33, 32))]
pub fn uint256_long_rejected() -> bool {
    true
}

fn main() {}
