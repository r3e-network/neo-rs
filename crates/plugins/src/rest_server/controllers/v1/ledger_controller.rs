// Copyright (C) 2015-2025 The Neo Project.
//
// LedgerController mirrors Neo.Plugins.RestServer.Controllers.v1.LedgerController.

use crate::rest_server::exceptions::{
    block_not_found_exception::BlockNotFoundException,
    invalid_parameter_range_exception::InvalidParameterRangeException,
    node_network_exception::NodeNetworkException,
    transaction_not_found_exception::TransactionNotFoundException,
    uint256_format_exception::UInt256FormatException,
};
use crate::rest_server::extensions::ledger_contract_extensions::LedgerContractExtensions;
use crate::rest_server::models::error::error_model::ErrorModel;
use crate::rest_server::models::ledger::MemoryPoolCountModel;
use crate::rest_server::rest_server_plugin::RestServerGlobals;
use crate::rest_server::rest_server_settings::RestServerSettings;
use crate::rest_server::RestServerUtility;
use neo_core::error::CoreError;
use neo_core::ledger::block::Block as LedgerBlock;
use neo_core::ledger::block_header::BlockHeader as LedgerBlockHeader;
use neo_core::network::p2p::payloads::block::Block as NetworkBlock;
use neo_core::network::p2p::payloads::header::Header as NetworkHeader;
use neo_core::network::p2p::payloads::witness::Witness as NetworkWitness;
use neo_core::smart_contract::native::ledger_contract::HashOrIndex;
use neo_core::smart_contract::native::{
    gas_token::GasToken, ledger_contract::LedgerContract, neo_token::NeoToken, NativeContract,
};
use neo_core::{NeoSystem, UInt256, Witness as LedgerWitness};
use serde_json::Value;
use std::cmp::Ordering;
use std::str::FromStr;
use std::sync::Arc;

pub struct LedgerController {
    neo_system: Arc<NeoSystem>,
}

impl LedgerController {
    pub fn new() -> Result<Self, ErrorModel> {
        RestServerGlobals::neo_system()
            .map(|system| Self { neo_system: system })
            .ok_or_else(|| NodeNetworkException::new().to_error_model())
    }

    pub fn gas_accounts(
        &self,
        page: Option<i32>,
        size: Option<i32>,
    ) -> Result<Option<Value>, ErrorModel> {
        self.token_accounts(
            GasToken::new().id(),
            GasToken::new().decimals() as i32,
            page,
            size,
        )
    }

    pub fn neo_accounts(
        &self,
        page: Option<i32>,
        size: Option<i32>,
    ) -> Result<Option<Value>, ErrorModel> {
        self.token_accounts(
            NeoToken::new().id(),
            NeoToken::new().decimals() as i32,
            page,
            size,
        )
    }

    pub fn blocks(
        &self,
        page: Option<i32>,
        size: Option<i32>,
    ) -> Result<Option<Value>, ErrorModel> {
        let (page, page_size) = Self::resolve_pagination(page, size)?;
        let store_cache = self.neo_system.store_cache();
        let ledger = LedgerContract::new();
        let current_index = ledger
            .current_index(&store_cache)
            .map_err(Self::map_core_error)?;

        let start = match current_index
            .checked_sub((page.saturating_sub(1) as u32).saturating_mul(page_size as u32))
        {
            Some(value) => value,
            None => return Ok(None),
        };

        let mut headers = Vec::new();
        let mut remaining = page_size as i64;
        let mut index = start as i64;
        let end = index - page_size as i64;

        while index > end && index >= 0 && remaining > 0 {
            match ledger
                .get_block(&store_cache, HashOrIndex::Index(index as u32))
                .map_err(Self::map_core_error)?
            {
                Some(block) => {
                    let header = Self::ledger_header_to_network(block.header);
                    headers.push(RestServerUtility::block_header_to_j_token(&header));
                }
                None => break,
            }
            if index == 0 {
                break;
            }
            index -= 1;
            remaining -= 1;
        }

        if headers.is_empty() {
            Ok(None)
        } else {
            serde_json::to_value(headers)
                .map(Some)
                .map_err(|err| Self::json_error(err.to_string()))
        }
    }

    pub fn current_block_header(&self) -> Result<Value, ErrorModel> {
        let store_cache = self.neo_system.store_cache();
        let ledger = LedgerContract::new();
        let current_index = ledger
            .current_index(&store_cache)
            .map_err(Self::map_core_error)?;
        let block = ledger
            .get_block(&store_cache, HashOrIndex::Index(current_index))
            .map_err(Self::map_core_error)?
            .ok_or_else(|| BlockNotFoundException::new(current_index).to_error_model())?;
        let header = Self::ledger_header_to_network(block.header);
        serde_json::to_value(RestServerUtility::block_header_to_j_token(&header))
            .map_err(|err| Self::json_error(err.to_string()))
    }

    pub fn block(&self, index: u32) -> Result<Value, ErrorModel> {
        let store_cache = self.neo_system.store_cache();
        let ledger = LedgerContract::new();
        let block = ledger
            .get_block(&store_cache, HashOrIndex::Index(index))
            .map_err(Self::map_core_error)?
            .ok_or_else(|| BlockNotFoundException::new(index).to_error_model())?;
        let network_block = Self::ledger_block_to_network(block);
        serde_json::to_value(RestServerUtility::block_to_j_token(&network_block))
            .map_err(|err| Self::json_error(err.to_string()))
    }

    pub fn block_header(&self, index: u32) -> Result<Value, ErrorModel> {
        let store_cache = self.neo_system.store_cache();
        let ledger = LedgerContract::new();
        let block = ledger
            .get_block(&store_cache, HashOrIndex::Index(index))
            .map_err(Self::map_core_error)?
            .ok_or_else(|| BlockNotFoundException::new(index).to_error_model())?;
        let header = Self::ledger_header_to_network(block.header);
        serde_json::to_value(RestServerUtility::block_header_to_j_token(&header))
            .map_err(|err| Self::json_error(err.to_string()))
    }

    pub fn block_witness(&self, index: u32) -> Result<Value, ErrorModel> {
        let store_cache = self.neo_system.store_cache();
        let ledger = LedgerContract::new();
        let block = ledger
            .get_block(&store_cache, HashOrIndex::Index(index))
            .map_err(Self::map_core_error)?
            .ok_or_else(|| BlockNotFoundException::new(index).to_error_model())?;
        let header = Self::ledger_header_to_network(block.header);
        serde_json::to_value(RestServerUtility::witness_to_j_token(&header.witness))
            .map_err(|err| Self::json_error(err.to_string()))
    }

    pub fn block_transactions(
        &self,
        index: u32,
        page: Option<i32>,
        size: Option<i32>,
    ) -> Result<Option<Value>, ErrorModel> {
        let (page, page_size) = Self::resolve_pagination(page, size)?;
        let store_cache = self.neo_system.store_cache();
        let ledger = LedgerContract::new();
        let block = ledger
            .get_block(&store_cache, HashOrIndex::Index(index))
            .map_err(Self::map_core_error)?
            .ok_or_else(|| BlockNotFoundException::new(index).to_error_model())?;

        if block.transactions.is_empty() {
            return Ok(None);
        }

        let values: Vec<Value> = block
            .transactions
            .iter()
            .map(RestServerUtility::transaction_to_j_token)
            .collect();

        Ok(Self::paginate_values(values, page, page_size))
    }

    pub fn transaction(&self, hash: &str) -> Result<Value, ErrorModel> {
        let hash = Self::parse_hash(hash)?;
        let store_cache = self.neo_system.store_cache();
        let ledger = LedgerContract::new();
        let state = ledger
            .get_transaction_state(&store_cache, &hash)
            .map_err(Self::map_core_error)?
            .ok_or_else(|| TransactionNotFoundException::new(hash).to_error_model())?;
        serde_json::to_value(RestServerUtility::transaction_to_j_token(
            state.transaction(),
        ))
        .map_err(|err| Self::json_error(err.to_string()))
    }

    pub fn transaction_witnesses(&self, hash: &str) -> Result<Value, ErrorModel> {
        let hash = Self::parse_hash(hash)?;
        let store_cache = self.neo_system.store_cache();
        let ledger = LedgerContract::new();
        let state = ledger
            .get_transaction_state(&store_cache, &hash)
            .map_err(Self::map_core_error)?
            .ok_or_else(|| TransactionNotFoundException::new(hash).to_error_model())?;
        let witnesses: Vec<Value> = state
            .transaction()
            .witnesses()
            .iter()
            .map(RestServerUtility::witness_to_j_token)
            .collect();
        serde_json::to_value(witnesses).map_err(|err| Self::json_error(err.to_string()))
    }

    pub fn transaction_signers(&self, hash: &str) -> Result<Value, ErrorModel> {
        let hash = Self::parse_hash(hash)?;
        let store_cache = self.neo_system.store_cache();
        let ledger = LedgerContract::new();
        let state = ledger
            .get_transaction_state(&store_cache, &hash)
            .map_err(Self::map_core_error)?
            .ok_or_else(|| TransactionNotFoundException::new(hash).to_error_model())?;
        let signers: Vec<Value> = state
            .transaction()
            .signers()
            .iter()
            .map(RestServerUtility::signer_to_j_token)
            .collect();
        serde_json::to_value(signers).map_err(|err| Self::json_error(err.to_string()))
    }

    pub fn transaction_attributes(&self, hash: &str) -> Result<Value, ErrorModel> {
        let hash = Self::parse_hash(hash)?;
        let store_cache = self.neo_system.store_cache();
        let ledger = LedgerContract::new();
        let state = ledger
            .get_transaction_state(&store_cache, &hash)
            .map_err(Self::map_core_error)?
            .ok_or_else(|| TransactionNotFoundException::new(hash).to_error_model())?;
        let attributes: Vec<Value> = state
            .transaction()
            .attributes()
            .iter()
            .map(RestServerUtility::transaction_attribute_to_j_token)
            .collect();
        serde_json::to_value(attributes).map_err(|err| Self::json_error(err.to_string()))
    }

    pub fn memory_pool(&self, page: Option<i32>, size: Option<i32>) -> Result<Value, ErrorModel> {
        let (page, page_size) = Self::resolve_pagination(page, size)?;
        let pool = self.neo_system.mempool();
        let guard = pool.lock().map_err(|err| {
            Self::internal_error(format!("Failed to acquire mempool lock: {err}"))
        })?;
        let transactions = guard.all_transactions_vec();
        let values: Vec<Value> = transactions
            .iter()
            .map(RestServerUtility::transaction_to_j_token)
            .collect();
        let paged =
            Self::paginate_values(values, page, page_size).unwrap_or_else(|| Value::Array(vec![]));
        Ok(paged)
    }

    pub fn memory_pool_verified(
        &self,
        page: Option<i32>,
        size: Option<i32>,
    ) -> Result<Option<Value>, ErrorModel> {
        let (page, page_size) = Self::resolve_pagination(page, size)?;
        let pool = self.neo_system.mempool();
        let guard = pool.lock().map_err(|err| {
            Self::internal_error(format!("Failed to acquire mempool lock: {err}"))
        })?;
        if guard.count() == 0 {
            return Ok(None);
        }
        let values: Vec<Value> = guard
            .verified_transactions_vec()
            .iter()
            .map(RestServerUtility::transaction_to_j_token)
            .collect();
        Ok(Self::paginate_values(values, page, page_size))
    }

    pub fn memory_pool_unverified(
        &self,
        page: Option<i32>,
        size: Option<i32>,
    ) -> Result<Option<Value>, ErrorModel> {
        let (page, page_size) = Self::resolve_pagination(page, size)?;
        let pool = self.neo_system.mempool();
        let guard = pool.lock().map_err(|err| {
            Self::internal_error(format!("Failed to acquire mempool lock: {err}"))
        })?;
        if guard.count() == 0 {
            return Ok(None);
        }
        let values: Vec<Value> = guard
            .unverified_transactions_vec()
            .iter()
            .map(RestServerUtility::transaction_to_j_token)
            .collect();
        Ok(Self::paginate_values(values, page, page_size))
    }

    pub fn memory_pool_counts(&self) -> Result<MemoryPoolCountModel, ErrorModel> {
        let pool = self.neo_system.mempool();
        let guard = pool.lock().map_err(|err| {
            Self::internal_error(format!("Failed to acquire mempool lock: {err}"))
        })?;
        Ok(MemoryPoolCountModel::new(
            guard.count(),
            guard.unverified_count(),
            guard.verified_count(),
        ))
    }

    fn token_accounts(
        &self,
        token_id: i32,
        decimals: i32,
        page: Option<i32>,
        size: Option<i32>,
    ) -> Result<Option<Value>, ErrorModel> {
        let (page, page_size) = Self::resolve_pagination(page, size)?;
        let store_cache = self.neo_system.store_cache();
        let settings = self.neo_system.settings();
        let address_version = settings.address_version;
        let mut accounts = LedgerContractExtensions::list_accounts(
            &store_cache,
            token_id,
            decimals,
            address_version,
        )
        .map_err(|err| Self::internal_error(err))?;
        if accounts.is_empty() {
            return Ok(None);
        }

        accounts.sort_by(|a, b| match b.balance.cmp(&a.balance) {
            Ordering::Equal => a.script_hash.cmp(&b.script_hash),
            other => other,
        });

        let paged = Self::paginate_values(
            accounts
                .into_iter()
                .map(|account| serde_json::to_value(account))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| Self::json_error(err.to_string()))?,
            page,
            page_size,
        );

        match paged {
            Some(Value::Array(ref arr)) if arr.is_empty() => Ok(None),
            other => Ok(other),
        }
    }

    fn ledger_block_to_network(block: LedgerBlock) -> NetworkBlock {
        NetworkBlock {
            header: Self::ledger_header_to_network(block.header),
            transactions: block.transactions,
        }
    }

    fn ledger_header_to_network(header: LedgerBlockHeader) -> NetworkHeader {
        let mut network_header = NetworkHeader::new();
        network_header.set_version(header.version);
        network_header.set_prev_hash(header.previous_hash);
        network_header.set_merkle_root(header.merkle_root);
        network_header.set_timestamp(header.timestamp);
        network_header.set_nonce(header.nonce);
        network_header.set_index(header.index);
        network_header.set_primary_index(header.primary_index);
        network_header.set_next_consensus(header.next_consensus);
        let witness = header
            .witnesses
            .into_iter()
            .next()
            .map(|w: LedgerWitness| {
                NetworkWitness::new_with_scripts(w.invocation_script, w.verification_script)
            })
            .unwrap_or_else(NetworkWitness::new);
        network_header.witness = witness;
        network_header
    }

    fn resolve_pagination(
        page: Option<i32>,
        size: Option<i32>,
    ) -> Result<(usize, usize), ErrorModel> {
        let settings = RestServerSettings::current();
        let max_page = settings.max_page_size as i32;
        let page = page.unwrap_or(1);
        let size = size.unwrap_or(50);
        if page < 1 || size < 1 || size > max_page {
            return Err(InvalidParameterRangeException::new().to_error_model());
        }
        Ok((page as usize, size as usize))
    }

    fn paginate_values(values: Vec<Value>, page: usize, page_size: usize) -> Option<Value> {
        if values.is_empty() {
            return None;
        }
        let offset = (page.saturating_sub(1)) * page_size;
        if offset >= values.len() {
            return None;
        }
        let end = (offset + page_size).min(values.len());
        let slice = values[offset..end].to_vec();
        Some(Value::Array(slice))
    }

    fn parse_hash(hash: &str) -> Result<UInt256, ErrorModel> {
        UInt256::from_str(hash).map_err(|_| UInt256FormatException::new().to_error_model())
    }

    fn map_core_error(error: CoreError) -> ErrorModel {
        Self::internal_error(error.to_string())
    }

    fn internal_error(message: impl Into<String>) -> ErrorModel {
        ErrorModel::with_params(
            crate::rest_server::exceptions::rest_error_codes::RestErrorCodes::GENERIC_EXCEPTION,
            "LedgerException".to_string(),
            message.into(),
        )
    }

    fn json_error(message: String) -> ErrorModel {
        Self::internal_error(format!("Failed to serialise response: {message}"))
    }
}
