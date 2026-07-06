//! Notary genesis/activation initialization.
//!
//! Seeds the notary setting introduced at HF_Echidna while keeping the root
//! module focused on identity, activation metadata, hooks, and dispatch.

use super::{DEFAULT_MAX_NOT_VALID_BEFORE_DELTA, Notary};
use neo_error::CoreResult;
use neo_execution::ApplicationEngine;
use neo_storage::StorageItem;
use num_bigint::BigInt;

impl Notary {
    /// C# `Notary.InitializeAsync(engine, hardfork)` for `hardfork == ActiveIn`
    /// (Notary.cs:52-59; ActiveIn is HF_Echidna, so this runs while persisting
    /// the Echidna activation block): seed `Prefix_MaxNotValidBeforeDelta` with
    /// `DefaultMaxNotValidBeforeDelta` (140).
    pub(super) fn initialize_native(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        engine.snapshot_cache().add(
            Self::max_not_valid_before_delta_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_MAX_NOT_VALID_BEFORE_DELTA,
            ))),
        );
        Ok(())
    }
}
