//! Oracle pricing (setPrice).
//!
//! Migrated from `neo-core/src/smart_contract/native/oracle_contract/pricing.rs`.
//! Import adaptation: `crate::error::*` → `neo_smart_contract::error::*`;
//! `crate::smart_contract::StorageItem` → `neo_storage::StorageItem`;
//! `crate::smart_contract::application_engine::*` →
//! `neo_smart_contract::application_engine::*`.

use super::OracleContract;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::error::{CoreError as Error, CoreResult as Result};
use neo_storage::StorageItem;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

impl OracleContract {
    pub(super) fn set_price(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::invalid_operation(
                "setPrice requires 1 argument".to_string(),
            ));
        }

        let price = BigInt::from_signed_bytes_le(&args[0])
            .to_i64()
            .ok_or_else(|| Error::invalid_operation("Invalid price value"))?;

        if price <= 0 {
            return Err(Error::invalid_operation(
                "Price must be positive".to_string(),
            ));
        }

        if !engine
            .check_committee_witness()
            .map_err(|err| Error::runtime_error(err.to_string()))?
        {
            return Err(Error::invalid_operation(
                "Committee authorization required".to_string(),
            ));
        }

        let snapshot_arc = engine.snapshot_cache();
        let snapshot = snapshot_arc.as_ref();
        self.put_item(
            snapshot,
            self.price_key(),
            StorageItem::from_bytes(BigInt::from(price).to_signed_bytes_le()),
        );

        Ok(Vec::new())
    }
}
