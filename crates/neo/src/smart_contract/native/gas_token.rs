use super::fungible_token::PREFIX_ACCOUNT as ACCOUNT_PREFIX;
use super::native_contract::{NativeContract, NativeMethod};
use crate::error::{CoreError, CoreResult};
use crate::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::{StorageItem, StorageKey};
use crate::UInt160;
use lazy_static::lazy_static;
use num_bigint::BigInt;
use num_traits::Zero;
use std::any::Any;

lazy_static! {
    static ref GAS_HASH: UInt160 = Helper::get_contract_hash(&UInt160::zero(), 0, "GasToken");
}

/// Simplified GAS native contract exposing canonical metadata. The
/// full implementation (including minting, burning, and distribution)
/// will be added once the supporting runtime subsystems are ported.
pub struct GasToken {
    methods: Vec<NativeMethod>,
}

impl GasToken {
    const ID: i32 = -6;
    const SYMBOL: &'static str = "GAS";
    const DECIMALS: u8 = 8;
    const NAME: &'static str = "GasToken";

    pub fn new() -> Self {
        let methods = vec![
            NativeMethod::safe("symbol".to_string(), 1),
            NativeMethod::safe("decimals".to_string(), 1),
        ];

        Self { methods }
    }

    fn invoke_method(&self, method: &str) -> CoreResult<Vec<u8>> {
        match method {
            "symbol" => Ok(Self::SYMBOL.as_bytes().to_vec()),
            "decimals" => Ok(vec![Self::DECIMALS]),
            _ => Err(CoreError::native_contract(format!(
                "Method not implemented: {}",
                method
            ))),
        }
    }

    pub fn symbol(&self) -> &'static str {
        Self::SYMBOL
    }

    pub fn decimals(&self) -> u8 {
        Self::DECIMALS
    }

    /// GAS is inflationary; its supply depends on protocol settings and runtime
    /// conditions. Until the full economic model is ported we surface a zero
    /// placeholder for callers that inspect the context directly.
    pub fn total_supply_placeholder(&self) -> BigInt {
        BigInt::from(0)
    }

    /// Reads the balance of `account` from the underlying snapshot, falling back to zero when no
    /// account state exists. Mirrors the semantics of the C# `BalanceOf` helper without requiring
    /// a full ApplicationEngine context.
    pub fn balance_of_snapshot<S>(&self, snapshot: &S, account: &UInt160) -> BigInt
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = StorageKey::create_with_uint160(Self::ID, ACCOUNT_PREFIX, account);
        snapshot
            .try_get(&key)
            .map(|item| item.to_bigint())
            .unwrap_or_else(BigInt::zero)
    }
}

impl NativeContract for GasToken {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *GAS_HASH
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn is_active(&self, _settings: &ProtocolSettings, _block_height: u32) -> bool {
        true
    }

    fn invoke(
        &self,
        _engine: &mut ApplicationEngine,
        method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_method(method)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
