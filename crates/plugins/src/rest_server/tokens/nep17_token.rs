//! Rust port of `Neo.Plugins.RestServer.Tokens.NEP17Token`.

use super::TokenError;
use crate::rest_server::helpers::{contract_helper::ContractHelper, script_helper::ScriptHelper};
use crate::rest_server::models::token::nep17_token_model::Nep17TokenModel;
use neo_core::big_decimal::BigDecimal;
use neo_core::neo_system::NeoSystem;
use neo_core::persistence::data_cache::DataCache;
use neo_core::UInt160;
use neo_vm::stack_item::StackItem;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::sync::Arc;

/// NEP-17 token helper mirroring the behaviour of the C# implementation.
pub struct Nep17Token {
    pub script_hash: UInt160,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    neo_system: Arc<NeoSystem>,
    snapshot: Arc<DataCache>,
}

impl Nep17Token {
    /// Creates a new NEP-17 token wrapper, validating support up-front.
    pub fn new(
        neo_system: Arc<NeoSystem>,
        script_hash: UInt160,
        snapshot: Option<Arc<DataCache>>,
    ) -> Result<Self, TokenError> {
        let snapshot = snapshot.unwrap_or_else(|| default_snapshot(&neo_system));
        let contract = ContractHelper::get_contract_state(snapshot.as_ref(), &script_hash)
            .map_err(TokenError::Storage)?
            .ok_or(TokenError::ContractNotFound(script_hash))?;

        if !ContractHelper::is_nep17_supported_contract(&contract) {
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

    /// Gets the balance of the specified account.
    pub fn balance_of(&self, address: &UInt160) -> Result<BigDecimal, TokenError> {
        if ContractHelper::get_contract_method(
            self.snapshot.as_ref(),
            &self.script_hash,
            "balanceOf",
            1,
        )
        .is_none()
        {
            return Err(TokenError::NotSupported(self.script_hash));
        }

        let arg = StackItem::from_byte_string(address.to_bytes());
        let protocol_settings = self.neo_system.settings();

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

    /// Gets the total supply reported by the contract.
    pub fn total_supply(&self) -> Result<BigDecimal, TokenError> {
        if ContractHelper::get_contract_method(
            self.snapshot.as_ref(),
            &self.script_hash,
            "totalSupply",
            0,
        )
        .is_none()
        {
            return Err(TokenError::NotSupported(self.script_hash));
        }

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

    /// Projects the token into the REST model.
    pub fn to_model(&self) -> Result<Nep17TokenModel, TokenError> {
        let total_supply = self.total_supply()?;
        Ok(Nep17TokenModel::new(
            self.name.clone(),
            self.script_hash,
            self.symbol.clone(),
            self.decimals,
            total_supply.value().clone(),
        ))
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
