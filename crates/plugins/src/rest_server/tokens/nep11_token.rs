//! Rust port of `Neo.Plugins.RestServer.Tokens.NEP11Token`.

use super::TokenError;
use crate::rest_server::helpers::{contract_helper::ContractHelper, script_helper::ScriptHelper};
use crate::rest_server::models::token::{
    nep11_token_model::Nep11TokenModel, nep17_token_model::Nep17TokenModel,
};
use crate::rest_server::newtonsoft::json::stack_item_json_converter::StackItemJsonConverter;
use hex::encode_upper;
use neo_core::big_decimal::BigDecimal;
use neo_core::neo_system::NeoSystem;
use neo_core::persistence::data_cache::DataCache;
use neo_core::UInt160;
use neo_vm::stack_item::StackItem;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

/// NEP-11 token helper mirroring the behaviour of the C# implementation.
pub struct Nep11Token {
    pub script_hash: UInt160,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    neo_system: Arc<NeoSystem>,
    snapshot: Arc<DataCache>,
}

impl Nep11Token {
    pub fn new(
        neo_system: Arc<NeoSystem>,
        script_hash: UInt160,
        snapshot: Option<Arc<DataCache>>,
    ) -> Result<Self, TokenError> {
        let snapshot = snapshot.unwrap_or_else(|| default_snapshot(&neo_system));
        let contract = ContractHelper::get_contract_state(snapshot.as_ref(), &script_hash)
            .map_err(TokenError::Storage)?
            .ok_or(TokenError::ContractNotFound(script_hash))?;

        if !ContractHelper::is_nep11_supported_contract(&contract) {
            return Err(TokenError::NotSupported(script_hash));
        }

        let protocol_settings = neo_system.settings();

        let decimals = invoke_decimals(protocol_settings, Arc::clone(&snapshot), &script_hash)?;
        let symbol = invoke_symbol(protocol_settings, Arc::clone(&snapshot), &script_hash)?;

        Ok(Self {
            script_hash,
            name: contract.manifest.name.clone(),
            symbol,
            decimals,
            neo_system,
            snapshot,
        })
    }

    pub fn total_supply(&self) -> Result<BigDecimal, TokenError> {
        let protocol_settings = self.neo_system.settings();

        let (halted, mut stack) = ScriptHelper::invoke_method(
            protocol_settings,
            Arc::clone(&self.snapshot),
            &self.script_hash,
            "totalSupply",
            &[],
        )?;

        if !halted {
            return Err(TokenError::InvocationFault {
                method: "totalSupply",
                message: "VM fault".to_string(),
            });
        }

        let amount = stack
            .pop()
            .map(extract_integer)
            .transpose()?
            .unwrap_or_else(|| BigInt::from(0));

        Ok(BigDecimal::new(amount, self.decimals))
    }

    pub fn balance_of(&self, owner: &UInt160) -> Result<BigDecimal, TokenError> {
        let protocol_settings = self.neo_system.settings();

        let arg = StackItem::from_byte_string(owner.to_bytes());
        let (halted, mut stack) = ScriptHelper::invoke_method(
            protocol_settings,
            Arc::clone(&self.snapshot),
            &self.script_hash,
            "balanceOf",
            &[arg],
        )?;

        if !halted {
            return Err(TokenError::InvocationFault {
                method: "balanceOf",
                message: "VM fault".to_string(),
            });
        }

        let amount = stack
            .pop()
            .map(extract_integer)
            .transpose()?
            .unwrap_or_else(|| BigInt::from(0));

        Ok(BigDecimal::new(amount, self.decimals))
    }

    pub fn balance_of_token(
        &self,
        owner: &UInt160,
        token_id: &[u8],
    ) -> Result<BigDecimal, TokenError> {
        if self.decimals == 0 {
            return Err(TokenError::InvocationFault {
                method: "balanceOf",
                message: "contract reports zero decimals for fractional balance query".to_string(),
            });
        }

        if token_id.len() > 64 {
            return Err(TokenError::Stack(
                "tokenId length exceeds 64 bytes".to_string(),
            ));
        }

        let protocol_settings = self.neo_system.settings();
        let args = [
            StackItem::from_byte_string(owner.to_bytes()),
            StackItem::from_byte_string(token_id.to_vec()),
        ];

        let (halted, mut stack) = ScriptHelper::invoke_method(
            protocol_settings,
            Arc::clone(&self.snapshot),
            &self.script_hash,
            "balanceOf",
            &args,
        )?;

        if !halted {
            return Err(TokenError::InvocationFault {
                method: "balanceOf",
                message: "VM fault".to_string(),
            });
        }

        let amount = stack
            .pop()
            .map(extract_integer)
            .transpose()?
            .unwrap_or_else(|| BigInt::from(0));

        Ok(BigDecimal::new(amount, self.decimals))
    }

    pub fn tokens(&self) -> Result<Vec<Vec<u8>>, TokenError> {
        self.invoke_iterator_method("tokens", &[])
    }

    pub fn tokens_of(&self, owner: &UInt160) -> Result<Vec<Vec<u8>>, TokenError> {
        let arg = StackItem::from_byte_string(owner.to_bytes());
        self.invoke_iterator_method("tokensOf", &[arg])
    }

    pub fn owner_of(&self, token_id: &[u8]) -> Result<Vec<UInt160>, TokenError> {
        let protocol_settings = self.neo_system.settings();
        let arg = StackItem::from_byte_string(token_id.to_vec());
        let (mut engine, stack, halted) = ScriptHelper::invoke_method_with_engine(
            protocol_settings,
            Arc::clone(&self.snapshot),
            &self.script_hash,
            "ownerOf",
            &[arg],
        )?;

        if !halted {
            return Err(TokenError::InvocationFault {
                method: "ownerOf",
                message: "VM fault".to_string(),
            });
        }

        let iterator_item = stack
            .first()
            .ok_or_else(|| TokenError::Stack("ownerOf iterator missing".to_string()))?;
        let iterator_id = iterator_identifier(iterator_item)?;

        let mut owners = Vec::new();
        while engine
            .iterator_next_internal(iterator_id)
            .map_err(storage_error)?
        {
            let value = engine
                .iterator_value_internal(iterator_id)
                .map_err(storage_error)?;
            match value {
                StackItem::ByteString(bytes) => {
                    let owner = UInt160::from_bytes(&bytes)
                        .map_err(|err| TokenError::Stack(err.to_string()))?;
                    owners.push(owner);
                }
                _ => {
                    return Err(TokenError::Stack(
                        "ownerOf iterator produced non-byte token".to_string(),
                    ));
                }
            }
        }

        let _ = engine.dispose_iterator(iterator_id);

        Ok(owners)
    }

    pub fn properties(
        &self,
        token_id: &[u8],
    ) -> Result<Option<BTreeMap<String, Value>>, TokenError> {
        if ContractHelper::get_contract_method(
            self.snapshot.as_ref(),
            &self.script_hash,
            "properties",
            1,
        )
        .is_none()
        {
            return Err(TokenError::NotSupported(self.script_hash));
        }

        if token_id.len() > 64 {
            return Err(TokenError::Stack(
                "tokenId length exceeds 64 bytes".to_string(),
            ));
        }

        let arg = StackItem::from_byte_string(token_id.to_vec());
        let (halted, mut stack) = ScriptHelper::invoke_method(
            self.neo_system.settings(),
            Arc::clone(&self.snapshot),
            &self.script_hash,
            "properties",
            &[arg],
        )?;

        if !halted {
            return Err(TokenError::InvocationFault {
                method: "properties",
                message: "VM fault".to_string(),
            });
        }

        let Some(item) = stack.pop() else {
            return Ok(None);
        };

        match item {
            StackItem::Null => Ok(None),
            StackItem::Map(map) => {
                let mut result = BTreeMap::new();
                for (key, value) in map.items() {
                    let key_hex = match key {
                        StackItem::ByteString(bytes) => encode_upper(bytes),
                        _ => {
                            return Err(TokenError::Stack(
                                "properties key must be byte string".to_string(),
                            ));
                        }
                    };
                    let json_value = StackItemJsonConverter::to_json(value)
                        .map_err(|err| TokenError::Stack(err.to_string()))?;
                    result.insert(key_hex, json_value);
                }
                Ok(Some(result))
            }
            other => Err(TokenError::Stack(format!(
                "properties returned unsupported stack item: {:?}",
                other.stack_item_type()
            ))),
        }
    }

    pub fn to_model(&self) -> Result<Nep11TokenModel, TokenError> {
        let total_supply = self.total_supply()?;
        let base = Nep17TokenModel::new(
            self.name.clone(),
            self.script_hash,
            self.symbol.clone(),
            self.decimals,
            total_supply.value().clone(),
        );

        let mut tokens = BTreeMap::new();
        if let Ok(token_ids) = self.tokens() {
            for token_id in token_ids {
                let key = encode_upper(&token_id);
                let properties = self.properties(&token_id)?;
                tokens.insert(key, properties);
            }
        }

        Ok(Nep11TokenModel::new(base, tokens))
    }

    fn invoke_iterator_method(
        &self,
        method: &'static str,
        args: &[StackItem],
    ) -> Result<Vec<Vec<u8>>, TokenError> {
        let protocol_settings = self.neo_system.settings();
        let (mut engine, stack, halted) = ScriptHelper::invoke_method_with_engine(
            protocol_settings,
            Arc::clone(&self.snapshot),
            &self.script_hash,
            method,
            args,
        )?;

        if !halted {
            return Err(TokenError::InvocationFault {
                method,
                message: "VM fault".to_string(),
            });
        }

        let iterator_item = stack
            .first()
            .ok_or_else(|| TokenError::Stack("iterator result missing".to_string()))?;
        let iterator_id = iterator_identifier(iterator_item)?;

        let mut values = Vec::new();
        while engine
            .iterator_next_internal(iterator_id)
            .map_err(storage_error)?
        {
            let value = engine
                .iterator_value_internal(iterator_id)
                .map_err(storage_error)?;
            match value {
                StackItem::ByteString(bytes) => values.push(bytes),
                StackItem::Buffer(buffer) => values.push(buffer.data().to_vec()),
                other => {
                    return Err(TokenError::Stack(format!(
                        "iterator returned unsupported item: {:?}",
                        other.stack_item_type()
                    )));
                }
            }
        }

        let _ = engine.dispose_iterator(iterator_id);

        Ok(values)
    }
}

fn invoke_decimals(
    settings: &neo_core::neo_system::ProtocolSettings,
    snapshot: Arc<DataCache>,
    script_hash: &UInt160,
) -> Result<u8, TokenError> {
    let (halted, mut stack) =
        ScriptHelper::invoke_method(settings, snapshot, script_hash, "decimals", &[])?;

    if !halted {
        return Err(TokenError::InvocationFault {
            method: "decimals",
            message: "VM fault".to_string(),
        });
    }

    let Some(item) = stack.pop() else {
        return Err(TokenError::Stack("decimals stack empty".to_string()));
    };

    let value = extract_integer(item)?;
    value
        .to_u8()
        .ok_or_else(|| TokenError::Stack("decimals value out of range".to_string()))
}

fn invoke_symbol(
    settings: &neo_core::neo_system::ProtocolSettings,
    snapshot: Arc<DataCache>,
    script_hash: &UInt160,
) -> Result<String, TokenError> {
    let (halted, mut stack) =
        ScriptHelper::invoke_method(settings, snapshot, script_hash, "symbol", &[])?;

    if !halted {
        return Err(TokenError::InvocationFault {
            method: "symbol",
            message: "VM fault".to_string(),
        });
    }

    let Some(item) = stack.pop() else {
        return Err(TokenError::Stack("symbol stack empty".to_string()));
    };

    extract_string(item)
}

fn extract_integer(item: StackItem) -> Result<BigInt, TokenError> {
    if item.is_null() {
        return Ok(BigInt::from(0));
    }

    item.get_integer()
        .map_err(|err| TokenError::Stack(err.to_string()))
}

fn extract_string(item: StackItem) -> Result<String, TokenError> {
    if item.is_null() {
        return Ok(String::new());
    }

    let bytes = item
        .as_bytes()
        .map_err(|err| TokenError::Stack(err.to_string()))?;
    String::from_utf8(bytes).map_err(|err| TokenError::Stack(err.to_string()))
}

fn default_snapshot(system: &Arc<NeoSystem>) -> Arc<DataCache> {
    Arc::new(system.store_cache().data_cache().clone())
}

fn iterator_identifier(item: &StackItem) -> Result<u32, TokenError> {
    let integer = item
        .get_integer()
        .map_err(|err| TokenError::Stack(err.to_string()))?;
    integer
        .to_u32()
        .ok_or_else(|| TokenError::Stack("iterator identifier out of range".to_string()))
}

fn storage_error(message: String) -> TokenError {
    TokenError::Storage(message)
}
