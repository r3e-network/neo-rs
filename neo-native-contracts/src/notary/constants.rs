//! Notary protocol constants and storage prefixes.
//!
//! Centralizes deposit defaults and storage-prefix bytes so the contract root
//! stays focused on the native-contract surface.

/// C# `Notary.DefaultDepositDeltaTill`: the default lock-height delta applied to a
/// first deposit whose `till` the depositor isn't allowed to set itself.
pub(in crate::notary) const DEFAULT_DEPOSIT_DELTA_TILL: u32 = 5760;

/// Storage prefix for the max-NotValidBefore-delta setting (C#
/// `Notary.Prefix_MaxNotValidBeforeDelta`).
pub(in crate::notary) const PREFIX_MAX_NOT_VALID_BEFORE_DELTA: u8 = 10;
/// C# `Notary.DefaultMaxNotValidBeforeDelta`.
pub(in crate::notary) const DEFAULT_MAX_NOT_VALID_BEFORE_DELTA: i64 = 140;
/// C# `Notary.Prefix_Deposit` - per-account deposit (`Struct[Amount, Till]`).
pub(in crate::notary) const PREFIX_DEPOSIT: u8 = 1;
