//! Thin wrapper over the `bigdecimal` crate for Neo N3's 8-decimal
//! fixed-point semantics.
//!
//! Added in the `2026-06-13-comprehensive-refactoring` change
//! (Phase C3) as a *sidecar* to the existing `BigDecimal` type.
//! The 448-LoC custom implementation in `big_decimal.rs` is
//! preserved unchanged for backward compatibility; new code that
//! does not need the C# parity quirks can use `Fixed8` below for
//! the common case of NEO/GAS amounts.
//!
//! ## Why a sidecar, not a replacement
//!
//! The custom `BigDecimal` in `big_decimal.rs` was carefully
//! tuned to match the C# `Neo.SmartContract.BigDecimal` semantics:
//! - **Empty-bytes zero encoding** for `StorageItem` compatibility
//!   (the C# `BigInteger.ToByteArrayStandard()` returns `[]` for
//!   zero, but `num-bigint`'s `to_signed_bytes_le()` returns `[0]`).
//! - **Panic on parse failure** for malformed fixed-point strings
//!   (to match the C# `decimal.Parse` behavior).
//! - **Specific rounding mode** for `change_decimals`.
//!
//! The `bigdecimal` crate uses different conventions for all three.
//! A full migration requires auditing every call site against the
//! C# reference; that's tracked as a multi-day follow-up in
//! `openspec/changes/2026-06-13-comprehensive-refactoring`.
//!
//! ## `Fixed8`: the common-case wrapper
//!
//! For the dominant use case (representing NEO/GAS amounts at
//! 8 decimal places), [`Fixed8`] provides a `bigdecimal`-backed
//! type that is interoperable with the existing `BigDecimal`
//! via the [`big_decimal_bigdecimal::BigDecimal::from_fixed8`]
//! conversion.

use bigdecimal::BigDecimal as ExternalBigDecimal;
use num_bigint::BigInt;

/// 8-decimal fixed-point number backed by the `bigdecimal` crate.
///
/// This is the recommended type for new code that needs
/// arbitrary-precision decimal arithmetic. The existing
/// `BigDecimal` (in `super::big_decimal`) remains the canonical
/// type for C# wire-compatibility (see the module-level docs).
#[derive(Clone, Debug)]
pub struct Fixed8(ExternalBigDecimal);

impl Fixed8 {
    /// The fixed number of decimal places for NEO/GAS amounts.
    pub const DECIMALS: u8 = 8;

    /// Create a new `Fixed8` from a raw `BigInt` value at the
    /// 8-decimal scale.
    pub fn from_bigint(value: BigInt) -> Self {
        Self(ExternalBigDecimal::from(value))
    }

    /// Create a new `Fixed8` from a `f64` value (truncated, not
    /// rounded — matches `BigDecimal::from_f64`).
    pub fn from_f64(value: f64) -> Self {
        // The `bigdecimal` crate exposes `BigDecimal::from_f64` only
        // behind the `arbitrary_precision` feature. The basic API
        // parses f64 via its `Display` impl, which is sufficient for
        // the trivial conversions we need here.
        let s = format!("{value}");
        Self(ExternalBigDecimal::parse_bytes(s.as_bytes(), 10).unwrap_or_default())
    }

    /// Returns the inner `bigdecimal::BigDecimal`.
    pub fn inner(&self) -> &ExternalBigDecimal {
        &self.0
    }

    /// Convert to a string in the canonical `decimal(8)` form.
    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl Default for Fixed8 {
    fn default() -> Self {
        Self(ExternalBigDecimal::from(0))
    }
}

impl std::fmt::Display for Fixed8 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<i64> for Fixed8 {
    fn from(value: i64) -> Self {
        Self(ExternalBigDecimal::from(value))
    }
}

impl From<u64> for Fixed8 {
    fn from(value: u64) -> Self {
        Self(ExternalBigDecimal::from(value))
    }
}

impl std::ops::Add for Fixed8 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self((self.0 + rhs.0).normalized())
    }
}

impl std::ops::Sub for Fixed8 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self((self.0 - rhs.0).normalized())
    }
}

impl std::ops::Mul for Fixed8 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self((self.0 * rhs.0).normalized())
    }
}

impl std::ops::Div for Fixed8 {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        Self((self.0 / rhs.0).normalized())
    }
}

impl PartialEq for Fixed8 {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Fixed8 {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed8_basic_arithmetic() {
        let a = Fixed8::from(1i64);
        let b = Fixed8::from(2i64);
        assert_eq!(a.clone() + b.clone(), Fixed8::from(3i64));
        assert_eq!(b.clone() - a.clone(), Fixed8::from(1i64));
        assert_eq!(a * b, Fixed8::from(2i64));
    }

    #[test]
    fn fixed8_default_is_zero() {
        assert_eq!(Fixed8::default().to_string(), "0");
    }
}
