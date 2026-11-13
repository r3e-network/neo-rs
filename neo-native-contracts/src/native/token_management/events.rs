//! TokenManagement notification emitters.
//!
//! Migrated from `neo-core/src/smart_contract/native/token_management/events.rs`.
//!
//! Import adaptation:
//! - `crate::UInt160` -> `neo_primitives::UInt160`
//! - `crate::error::{CoreResult, ToNativeError}`
//!   -> `neo_smart_contract::error::{CoreResult, ToNativeError}`
//! - `crate::neo_vm::StackItem` -> `neo_vm::StackItem`
//! - `crate::smart_contract::application_engine::ApplicationEngine`
//!   -> `neo_smart_contract::application_engine::ApplicationEngine`
//! - `crate::smart_contract::native::NativeContract` -> `crate::native::NativeContract`

use super::{TokenManagement, TokenType};
use crate::native::NativeContract;
use neo_primitives::UInt160;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::error::{CoreResult, ToNativeError};
use neo_vm::StackItem;
use num_bigint::BigInt;

impl TokenManagement {
    pub(super) fn emit_transfer_event(
        &self,
        engine: &mut ApplicationEngine,
        from: Option<&UInt160>,
        to: Option<&UInt160>,
        amount: &BigInt,
    ) -> CoreResult<()> {
        let from_item = from.map_or_else(StackItem::null, |addr| {
            StackItem::from_byte_string(addr.to_bytes())
        });
        let to_item = to.map_or_else(StackItem::null, |addr| {
            StackItem::from_byte_string(addr.to_bytes())
        });
        let amount_item = StackItem::from_int(amount.clone());
        engine
            .send_notification(
                self.hash(),
                "Transfer".to_string(),
                vec![from_item, to_item, amount_item],
            )
            .native_err()
    }

    pub(super) fn emit_created_event(
        &self,
        engine: &mut ApplicationEngine,
        asset_id: &UInt160,
        token_type: &TokenType,
    ) -> CoreResult<()> {
        let type_value = match token_type {
            TokenType::Fungible => 0,
            TokenType::NonFungible => 1,
        };
        let type_item = StackItem::from_int(type_value);
        let asset_item = StackItem::from_byte_string(asset_id.to_bytes());
        engine
            .send_notification(
                self.hash(),
                "Created".to_string(),
                vec![asset_item, type_item],
            )
            .native_err()
    }
}
