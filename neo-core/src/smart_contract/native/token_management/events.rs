use super::{TokenManagement, TokenType};
use crate::error::{CoreResult, ToNativeError};
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::NativeContract;
use crate::neo_vm::{StackItem};
use crate::UInt160;
use num_bigint::BigInt;

impl TokenManagement {
    pub(super) fn emit_transfer_event(
        &self,
        engine: &mut ApplicationEngine,
        from: Option<&UInt160>,
        to: Option<&UInt160>,
        amount: &BigInt,
    ) -> CoreResult<()> {
        let from_item = from
            .map(|addr| StackItem::from_byte_string(addr.to_bytes()))
            .unwrap_or_else(StackItem::null);
        let to_item = to
            .map(|addr| StackItem::from_byte_string(addr.to_bytes()))
            .unwrap_or_else(StackItem::null);
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
