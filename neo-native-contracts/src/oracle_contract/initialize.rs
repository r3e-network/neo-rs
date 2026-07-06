//! OracleContract genesis initialization.
//!
//! Seeds the request counter and oracle price exactly as C# does while keeping
//! the root module focused on identity, metadata, provider seams, hooks, and
//! dispatch.

use super::{DEFAULT_ORACLE_PRICE, OracleContract};
use neo_error::CoreResult;
use neo_execution::ApplicationEngine;
use neo_storage::StorageItem;
use num_bigint::BigInt;

impl OracleContract {
    /// C# `OracleContract.InitializeAsync(engine, hardfork)` for
    /// `hardfork == ActiveIn` (the Oracle contract is genesis-active): seed the
    /// request-id counter with `BigInteger.Zero` (stored as empty bytes) and the
    /// request price with 0.5 GAS (`0_50000000` datoshi).
    pub(super) fn initialize_native(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let snapshot = engine.snapshot_cache();
        snapshot.add(
            Self::request_id_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(0))),
        );
        snapshot.add(
            Self::price_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_ORACLE_PRICE,
            ))),
        );
        Ok(())
    }
}
