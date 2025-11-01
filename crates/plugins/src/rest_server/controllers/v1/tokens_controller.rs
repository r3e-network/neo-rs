//! Rust port of `Neo.Plugins.RestServer.Controllers.v1.TokensController`.

use crate::rest_server::binder::uint160_binder_provider::UInt160BinderProvider;
use crate::rest_server::exceptions::contract_not_found_exception::ContractNotFoundException;
use crate::rest_server::exceptions::invalid_parameter_range_exception::InvalidParameterRangeException;
use crate::rest_server::exceptions::nep11_not_supported_exception::Nep11NotSupportedException;
use crate::rest_server::exceptions::nep17_not_supported_exception::Nep17NotSupportedException;
use crate::rest_server::exceptions::node_network_exception::NodeNetworkException;
use crate::rest_server::exceptions::rest_error_codes::RestErrorCodes;
use crate::rest_server::exceptions::script_hash_format_exception::ScriptHashFormatException;
use crate::rest_server::helpers::contract_helper::ContractHelper;
use crate::rest_server::models::error::error_model::ErrorModel;
use crate::rest_server::models::token::{
    nep11_token_model::Nep11TokenModel, nep17_token_model::Nep17TokenModel,
    token_balance_model::TokenBalanceModel,
};
use crate::rest_server::rest_server_plugin::RestServerGlobals;
use crate::rest_server::rest_server_settings::RestServerSettings;
use crate::rest_server::tokens::nep11_token::Nep11Token;
use crate::rest_server::tokens::nep17_token::Nep17Token;
use crate::rest_server::tokens::TokenError;
use neo_core::NeoSystem;
use neo_core::UInt160;
use num_traits::Zero;
use std::convert::TryFrom;
use std::sync::Arc;

pub struct TokensController {
    neo_system: Arc<NeoSystem>,
}

impl TokensController {
    pub fn new() -> Result<Self, ErrorModel> {
        RestServerGlobals::neo_system()
            .map(|system| Self { neo_system: system })
            .ok_or_else(|| NodeNetworkException::new().to_error_model())
    }

    pub fn get_nep17(
        &self,
        page: Option<i32>,
        size: Option<i32>,
    ) -> Result<Option<Vec<Nep17TokenModel>>, ErrorModel> {
        let (page, size) = Self::resolve_pagination(page, size)?;
        let store_cache = self.neo_system.store_cache();
        let contracts =
            ContractHelper::list_contracts(&store_cache).map_err(Self::storage_error)?;

        let mut supported: Vec<_> = contracts
            .into_iter()
            .filter(|contract| ContractHelper::is_nep17_supported_contract(contract))
            .collect();

        if supported.is_empty() {
            return Ok(None);
        }

        supported.sort_by_key(|contract| contract.id);

        let start = (page.saturating_sub(1) as usize).saturating_mul(size as usize);
        let mut models = Vec::new();

        for contract in supported.into_iter().skip(start).take(size as usize) {
            if let Ok(token) = Nep17Token::new(self.neo_system.clone(), contract.hash, None) {
                if let Ok(model) = token.to_model() {
                    models.push(model);
                }
            }
        }

        if models.is_empty() {
            Ok(None)
        } else {
            Ok(Some(models))
        }
    }

    pub fn get_nep17_balance_of(
        &self,
        token_hash: &str,
        address: &str,
    ) -> Result<TokenBalanceModel, ErrorModel> {
        let script_hash = Self::parse_script_hash(token_hash)?;
        let owner = Self::parse_script_hash(address)?;

        let store_cache = self.neo_system.store_cache();
        let contract = ContractHelper::get_contract_state(&store_cache, &script_hash)
            .map_err(Self::storage_error)?
            .ok_or_else(|| ContractNotFoundException::new(script_hash).to_error_model())?;

        if !ContractHelper::is_nep17_supported_contract(&contract) {
            return Err(Nep17NotSupportedException::new(script_hash).to_error_model());
        }

        let token = Nep17Token::new(self.neo_system.clone(), script_hash, None)
            .map_err(|error| Self::map_token_error(error, TokenStandard::Nep17))?;

        let balance = token
            .balance_of(&owner)
            .map_err(|error| Self::map_token_error(error, TokenStandard::Nep17))?;
        let total_supply = token
            .total_supply()
            .map_err(|error| Self::map_token_error(error, TokenStandard::Nep17))?;

        Ok(TokenBalanceModel::new(
            token.name.clone(),
            token.script_hash,
            token.symbol.clone(),
            token.decimals,
            balance.value().clone(),
            total_supply.value().clone(),
        ))
    }

    pub fn get_nep11(
        &self,
        page: Option<i32>,
        size: Option<i32>,
    ) -> Result<Option<Vec<Nep11TokenModel>>, ErrorModel> {
        let (page, size) = Self::resolve_pagination(page, size)?;
        let store_cache = self.neo_system.store_cache();
        let contracts =
            ContractHelper::list_contracts(&store_cache).map_err(Self::storage_error)?;

        let mut supported: Vec<_> = contracts
            .into_iter()
            .filter(|contract| ContractHelper::is_nep11_supported_contract(contract))
            .collect();

        if supported.is_empty() {
            return Ok(None);
        }

        supported.sort_by_key(|contract| contract.id);

        let start = (page.saturating_sub(1) as usize).saturating_mul(size as usize);
        let mut models = Vec::new();

        for contract in supported.into_iter().skip(start).take(size as usize) {
            if let Ok(token) = Nep11Token::new(self.neo_system.clone(), contract.hash, None) {
                if let Ok(model) = token.to_model() {
                    models.push(model);
                }
            }
        }

        if models.is_empty() {
            Ok(None)
        } else {
            Ok(Some(models))
        }
    }

    pub fn get_nep11_balance_of(
        &self,
        token_hash: &str,
        address: &str,
    ) -> Result<TokenBalanceModel, ErrorModel> {
        let script_hash = Self::parse_script_hash(token_hash)?;
        let owner = Self::parse_script_hash(address)?;

        let store_cache = self.neo_system.store_cache();
        let contract = ContractHelper::get_contract_state(&store_cache, &script_hash)
            .map_err(Self::storage_error)?
            .ok_or_else(|| ContractNotFoundException::new(script_hash).to_error_model())?;

        if !ContractHelper::is_nep11_supported_contract(&contract) {
            return Err(Nep11NotSupportedException::new(script_hash).to_error_model());
        }

        let token = Nep11Token::new(self.neo_system.clone(), script_hash, None)
            .map_err(|error| Self::map_token_error(error, TokenStandard::Nep11))?;

        let balance = token
            .balance_of(&owner)
            .map_err(|error| Self::map_token_error(error, TokenStandard::Nep11))?;
        let total_supply = token
            .total_supply()
            .map_err(|error| Self::map_token_error(error, TokenStandard::Nep11))?;

        Ok(TokenBalanceModel::new(
            token.name.clone(),
            token.script_hash,
            token.symbol.clone(),
            token.decimals,
            balance.value().clone(),
            total_supply.value().clone(),
        ))
    }

    pub fn get_balances(&self, address: &str) -> Result<Vec<TokenBalanceModel>, ErrorModel> {
        let owner = Self::parse_script_hash(address)?;
        let store_cache = self.neo_system.store_cache();
        let contracts =
            ContractHelper::list_contracts(&store_cache).map_err(Self::storage_error)?;

        let mut balances = Vec::new();

        for contract in contracts {
            // Attempt NEP-17 balance
            if ContractHelper::is_nep17_supported_contract(&contract) {
                if let Ok(token) = Nep17Token::new(self.neo_system.clone(), contract.hash, None) {
                    if let Ok(balance) = token.balance_of(&owner) {
                        if !balance.value().is_zero() {
                            if let Ok(total_supply) = token.total_supply() {
                                balances.push(TokenBalanceModel::new(
                                    token.name.clone(),
                                    token.script_hash,
                                    token.symbol.clone(),
                                    token.decimals,
                                    balance.value().clone(),
                                    total_supply.value().clone(),
                                ));
                            }
                        }
                    }
                }
            }

            // Attempt NEP-11 balance
            if ContractHelper::is_nep11_supported_contract(&contract) {
                if let Ok(token) = Nep11Token::new(self.neo_system.clone(), contract.hash, None) {
                    if let Ok(balance) = token.balance_of(&owner) {
                        if !balance.value().is_zero() {
                            if let Ok(total_supply) = token.total_supply() {
                                balances.push(TokenBalanceModel::new(
                                    token.name.clone(),
                                    token.script_hash,
                                    token.symbol.clone(),
                                    token.decimals,
                                    balance.value().clone(),
                                    total_supply.value().clone(),
                                ));
                            }
                        }
                    }
                }
            }
        }

        Ok(balances)
    }

    fn resolve_pagination(page: Option<i32>, size: Option<i32>) -> Result<(i32, i32), ErrorModel> {
        let settings = RestServerSettings::current();
        let max_size = i32::try_from(settings.max_page_size).unwrap_or(i32::MAX);
        let page = page.unwrap_or(1);
        let size = size.unwrap_or(max_size);

        if page < 1 || size < 1 || size > max_size {
            return Err(InvalidParameterRangeException::new().to_error_model());
        }

        Ok((page, size))
    }

    fn parse_script_hash(value: &str) -> Result<UInt160, ErrorModel> {
        UInt160BinderProvider::bind(value)
            .ok_or_else(|| ScriptHashFormatException::new().to_error_model())
    }

    fn storage_error(message: String) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::GENERIC_EXCEPTION,
            "StorageException".to_string(),
            message,
        )
    }

    fn map_token_error(error: TokenError, standard: TokenStandard) -> ErrorModel {
        match error {
            TokenError::ContractNotFound(hash) => {
                ContractNotFoundException::new(hash).to_error_model()
            }
            TokenError::NotSupported(hash) => match standard {
                TokenStandard::Nep17 => Nep17NotSupportedException::new(hash).to_error_model(),
                TokenStandard::Nep11 => Nep11NotSupportedException::new(hash).to_error_model(),
            },
            TokenError::InvocationFault { method, message } => ErrorModel::with_params(
                RestErrorCodes::GENERIC_EXCEPTION,
                format!("InvocationFault({method})"),
                message,
            ),
            TokenError::Stack(message) | TokenError::Storage(message) => ErrorModel::with_params(
                RestErrorCodes::GENERIC_EXCEPTION,
                "TokenError".to_string(),
                message,
            ),
            TokenError::Script(err) => ErrorModel::with_params(
                RestErrorCodes::GENERIC_EXCEPTION,
                "ScriptHelperError".to_string(),
                err.to_string(),
            ),
        }
    }
}

enum TokenStandard {
    Nep17,
    Nep11,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rest_server::exceptions::rest_error_codes::RestErrorCodes;

    #[test]
    fn resolve_pagination_defaults() {
        let (page, size) = TokensController::resolve_pagination(None, None).unwrap();
        assert_eq!(page, 1);
        assert!(size > 0);
    }

    #[test]
    fn resolve_pagination_rejects_invalid() {
        let err = TokensController::resolve_pagination(Some(0), Some(10)).unwrap_err();
        assert_eq!(err.code, RestErrorCodes::PARAMETER_FORMAT_EXCEPTION);
    }

    #[test]
    fn map_token_error_for_not_supported() {
        let hash = UInt160::zero();
        let error_model =
            TokensController::map_token_error(TokenError::NotSupported(hash), TokenStandard::Nep17);
        assert_eq!(error_model.code, RestErrorCodes::GENERIC_EXCEPTION);
        assert!(error_model.message.contains("does not support NEP-17"));
    }
}
