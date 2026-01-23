//! TokenManagement native contract implementation.
//!
//! This module provides the TokenManagement native contract which manages
//! token metadata and operations on the Neo blockchain.

use crate::cryptography::NeoHash;
use crate::error::CoreError;
use crate::error::CoreResult;
use crate::hardfork::Hardfork;
use crate::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use crate::persistence::seek_direction::SeekDirection;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::find_options::FindOptions;
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::iterators::StorageIterator;
use crate::smart_contract::native::NativeContract;
use crate::smart_contract::native::NativeMethod;
use crate::smart_contract::storage_context::StorageContext;
use crate::smart_contract::ContractParameterType;
use crate::smart_contract::StorageItem;
use crate::smart_contract::StorageKey;
use crate::UInt160;
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use neo_vm::StackItem;
use num_bigint::BigInt;
use num_traits::Signed;
use num_traits::ToPrimitive;
use num_traits::Zero;
use std::any::Any;

const ID: i32 = -12;
const PREFIX_TOKEN_STATE: u8 = 10;
const PREFIX_ACCOUNT_STATE: u8 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    Fungible = 0,
    NonFungible = 1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenState {
    pub token_type: TokenType,
    pub owner: UInt160,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub total_supply: BigInt,
    pub max_supply: BigInt,
    pub mintable_address: Option<UInt160>,
}

impl Default for TokenState {
    fn default() -> Self {
        Self {
            token_type: TokenType::Fungible,
            owner: UInt160::zero(),
            name: String::new(),
            symbol: String::new(),
            decimals: 0,
            total_supply: BigInt::from(0),
            max_supply: BigInt::from(0),
            mintable_address: None,
        }
    }
}

impl IInteroperable for TokenState {
    fn from_stack_item(&mut self, stack_item: StackItem) {
        if let StackItem::Struct(struct_item) = stack_item {
            let items = struct_item.items();
            if items.len() >= 7 {
                if let Ok(token_type_int) = items[0].as_int() {
                    self.token_type = if token_type_int == BigInt::from(1) {
                        TokenType::NonFungible
                    } else {
                        TokenType::Fungible
                    };
                }
                if let Ok(bytes) = items[1].as_bytes() {
                    if let Ok(owner) = UInt160::from_bytes(&bytes) {
                        self.owner = owner;
                    }
                }
                if let Ok(bytes) = items[2].as_bytes() {
                    self.name = String::from_utf8_lossy(&bytes).to_string();
                }
                if let Ok(bytes) = items[3].as_bytes() {
                    self.symbol = String::from_utf8_lossy(&bytes).to_string();
                }
                if let Ok(decimals_int) = items[4].as_int() {
                    self.decimals = decimals_int.to_u8().unwrap_or(0);
                }
                if let Ok(total_supply) = items[5].as_int() {
                    self.total_supply = total_supply;
                }
                if let Ok(max_supply) = items[6].as_int() {
                    self.max_supply = max_supply;
                }
                if items.len() >= 8 {
                    if let Ok(mintable) = items[7].get_boolean() {
                        if mintable {
                            self.mintable_address = Some(self.owner);
                        }
                    }
                }
            }
        }
    }

    fn to_stack_item(&self) -> StackItem {
        let mut items = Vec::new();
        items.push(StackItem::from_int(self.token_type as i32));
        items.push(StackItem::from_byte_string(self.owner.to_bytes()));
        items.push(StackItem::from_byte_string(self.name.as_bytes().to_vec()));
        items.push(StackItem::from_byte_string(self.symbol.as_bytes().to_vec()));
        items.push(StackItem::from_int(self.decimals as i32));
        items.push(StackItem::from_int(self.total_supply.clone()));
        items.push(StackItem::from_int(self.max_supply.clone()));
        items.push(StackItem::from_bool(self.mintable_address.is_some()));
        StackItem::from_struct(items)
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountState {
    pub balance: BigInt,
}

impl AccountState {
    pub fn new() -> Self {
        Self {
            balance: BigInt::from(0),
        }
    }

    pub fn with_balance(balance: BigInt) -> Self {
        Self { balance }
    }
}

impl IInteroperable for AccountState {
    fn from_stack_item(&mut self, stack_item: StackItem) {
        if let StackItem::Struct(struct_item) = stack_item {
            let items = struct_item.items();
            if let Some(first) = items.first() {
                if let Ok(integer) = first.as_int() {
                    self.balance = integer;
                }
            }
        }
    }

    fn to_stack_item(&self) -> StackItem {
        StackItem::from_struct(vec![StackItem::from_int(self.balance.clone())])
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NFTState {
    pub asset_id: UInt160,
    pub owner: UInt160,
    pub properties: Vec<(Vec<u8>, Vec<u8>)>,
}

impl NFTState {
    pub fn new() -> Self {
        Self::default()
    }
}

impl IInteroperable for NFTState {
    fn from_stack_item(&mut self, _stack_item: StackItem) {}

    fn to_stack_item(&self) -> StackItem {
        let properties_items: Vec<StackItem> = self
            .properties
            .iter()
            .map(|(k, v)| {
                StackItem::from_struct(vec![
                    StackItem::from_byte_string(k.clone()),
                    StackItem::from_byte_string(v.clone()),
                ])
            })
            .collect();
        StackItem::from_struct(vec![
            StackItem::from_byte_string(self.asset_id.to_bytes()),
            StackItem::from_byte_string(self.owner.to_bytes()),
            StackItem::from_array(properties_items),
        ])
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone)]
pub struct TokenManagement {
    methods: Vec<NativeMethod>,
}

impl TokenManagement {
    pub fn new() -> Self {
        let methods = vec![
            NativeMethod::new(
                "getTokenInfo".to_string(),
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::Array,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["assetId".to_string()]),
            NativeMethod::new(
                "balanceOf".to_string(),
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                ],
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["assetId".to_string(), "account".to_string()]),
            NativeMethod::new(
                "getAssetsOfOwner".to_string(),
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::Array,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["owner".to_string()]),
            NativeMethod::new(
                "create".to_string(),
                1 << 15,
                false,
                CallFlags::WRITE_STATES.bits() | CallFlags::ALLOW_CALL.bits(),
                vec![
                    ContractParameterType::Integer,
                    ContractParameterType::Hash160,
                    ContractParameterType::String,
                    ContractParameterType::String,
                    ContractParameterType::Integer,
                    ContractParameterType::Integer,
                    ContractParameterType::Boolean,
                ],
                ContractParameterType::Hash160,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec![
                "type".to_string(),
                "owner".to_string(),
                "name".to_string(),
                "symbol".to_string(),
                "decimals".to_string(),
                "maxSupply".to_string(),
                "mintable".to_string(),
            ]),
            NativeMethod::new(
                "createNonFungible".to_string(),
                1 << 15,
                false,
                CallFlags::WRITE_STATES.bits() | CallFlags::ALLOW_CALL.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::String,
                    ContractParameterType::String,
                    ContractParameterType::Boolean,
                ],
                ContractParameterType::Hash160,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec![
                "owner".to_string(),
                "name".to_string(),
                "symbol".to_string(),
                "mintable".to_string(),
            ]),
            NativeMethod::new(
                "mint".to_string(),
                1 << 15,
                false,
                CallFlags::WRITE_STATES.bits() | CallFlags::ALLOW_CALL.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["assetId".to_string(), "amountOrNftId".to_string()]),
            NativeMethod::new(
                "burn".to_string(),
                1 << 15,
                false,
                CallFlags::WRITE_STATES.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["assetId".to_string(), "amountOrNftId".to_string()]),
            NativeMethod::new(
                "transfer".to_string(),
                1 << 15,
                false,
                CallFlags::WRITE_STATES.bits() | CallFlags::ALLOW_CALL.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                    ContractParameterType::Any,
                ],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec![
                "assetId".to_string(),
                "from".to_string(),
                "to".to_string(),
                "amountOrNftId".to_string(),
                "data".to_string(),
            ]),
        ];
        Self { methods }
    }
}

impl Default for TokenManagement {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeContract for TokenManagement {
    fn id(&self) -> i32 {
        ID
    }

    fn hash(&self) -> UInt160 {
        UInt160::from([
            0xae, 0x00, 0xc5, 0x7d, 0xae, 0xb2, 0x0f, 0x9b, 0x65, 0x4f, 0x32, 0x65, 0xa9, 0x18,
            0xf4, 0x4a, 0x8a, 0x40, 0xe0, 0x49,
        ])
    }

    fn name(&self) -> &str {
        "TokenManagement"
    }

    fn active_in(&self) -> Option<Hardfork> {
        Some(Hardfork::HfFaun)
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn supported_standards(&self, _settings: &ProtocolSettings, _block_height: u32) -> Vec<String> {
        Vec::new()
    }

    fn activations(&self) -> Vec<Hardfork> {
        vec![Hardfork::HfFaun]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_method(engine, method, args)
    }
}

impl TokenManagement {
    fn get_token_state(
        &self,
        engine: &ApplicationEngine,
        asset_id: &UInt160,
    ) -> CoreResult<Option<TokenState>> {
        let snapshot = engine.snapshot_cache();
        let key = StorageKey::create_with_uint160(ID, PREFIX_TOKEN_STATE, asset_id);
        let Some(item) = snapshot.as_ref().try_get(&key) else {
            return Ok(None);
        };
        let bytes = item.get_value();
        if bytes.is_empty() {
            return Ok(None);
        }
        let stack_item =
            BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None)
                .map_err(CoreError::native_contract)?;
        let mut token_state = TokenState::default();
        token_state.from_stack_item(stack_item);
        Ok(Some(token_state))
    }

    fn get_account_state(
        &self,
        engine: &ApplicationEngine,
        asset_id: &UInt160,
        account: &UInt160,
    ) -> CoreResult<Option<AccountState>> {
        let snapshot = engine.snapshot_cache();
        let key = [
            vec![PREFIX_ACCOUNT_STATE],
            account.to_bytes().to_vec(),
            asset_id.to_bytes().to_vec(),
        ]
        .concat();
        let key = StorageKey::new(ID, key);
        let Some(item) = snapshot.as_ref().try_get(&key) else {
            return Ok(None);
        };
        let bytes = item.get_value();
        if bytes.is_empty() {
            return Ok(None);
        }
        let stack_item =
            BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None)
                .map_err(CoreError::native_contract)?;
        let mut account_state = AccountState::default();
        account_state.from_stack_item(stack_item);
        Ok(Some(account_state))
    }

    fn write_account_state(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        asset_id: &UInt160,
        state: &AccountState,
    ) -> CoreResult<()> {
        let key = [
            vec![PREFIX_ACCOUNT_STATE],
            account.to_bytes().to_vec(),
            asset_id.to_bytes().to_vec(),
        ]
        .concat();
        let key = StorageKey::new(ID, key);
        if state.balance.is_zero() {
            engine.delete_storage_item(context, &key.suffix().to_vec())?;
        } else {
            let stack_item = state.to_stack_item();
            let bytes = BinarySerializer::serialize(&stack_item, &ExecutionEngineLimits::default())
                .map_err(CoreError::native_contract)?;
            engine.put_storage_item(context, &key.suffix().to_vec(), &bytes)?;
        }
        Ok(())
    }

    fn emit_transfer_event(
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
            .map_err(CoreError::native_contract)
    }

    fn emit_created_event(
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
            .map_err(CoreError::native_contract)
    }

    fn get_asset_id(owner: &UInt160, name: &str) -> UInt160 {
        let name_bytes = name.as_bytes();
        let mut buffer = Vec::with_capacity(20 + name_bytes.len());
        buffer.extend_from_slice(&owner.as_bytes());
        buffer.extend_from_slice(name_bytes);
        let hash = NeoHash::hash160(&buffer);
        UInt160::from_bytes(&hash).unwrap_or_default()
    }

    pub fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match method {
            "getTokenInfo" => self.invoke_get_token_info(engine, args),
            "balanceOf" => self.invoke_balance_of(engine, args),
            "getAssetsOfOwner" => self.invoke_get_assets_of_owner(engine, args),
            "create" => self.invoke_create(engine, args),
            "createNonFungible" => self.invoke_create_non_fungible(engine, args),
            "mint" => self.invoke_mint(engine, args),
            "burn" => self.invoke_burn(engine, args),
            "transfer" => self.invoke_transfer(engine, args),
            _ => Err(CoreError::native_contract(format!(
                "TokenManagement: unknown method '{}'",
                method
            ))),
        }
    }

    fn invoke_get_token_info(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() < 1 {
            return Err(CoreError::native_contract(
                "TokenManagement.getTokenInfo: invalid arguments",
            ));
        }

        let asset_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid asset ID"))?;

        let Some(token_state) = self.get_token_state(engine, &asset_id)? else {
            return Err(CoreError::native_contract(
                "TokenManagement.getTokenInfo: token not found",
            ));
        };

        let stack_item = token_state.to_stack_item();
        let bytes = BinarySerializer::serialize(&stack_item, &ExecutionEngineLimits::default())
            .map_err(CoreError::native_contract)?;
        Ok(bytes)
    }

    fn invoke_balance_of(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() < 2 {
            return Err(CoreError::native_contract(
                "TokenManagement.balanceOf: invalid arguments",
            ));
        }

        let asset_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid asset ID"))?;
        let account = UInt160::from_bytes(&args[1])
            .map_err(|_| CoreError::native_contract("Invalid account"))?;

        let Some(account_state) = self.get_account_state(engine, &asset_id, &account)? else {
            return Ok(vec![0]);
        };

        let bytes = account_state.balance.to_signed_bytes_le();
        Ok(bytes)
    }

    fn invoke_get_assets_of_owner(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() < 1 {
            return Err(CoreError::native_contract(
                "TokenManagement.getAssetsOfOwner: invalid arguments",
            ));
        }

        let account = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid account"))?;

        let snapshot = engine.snapshot_cache();
        let prefix = StorageKey::create(ID, PREFIX_ACCOUNT_STATE);

        let mut entries_map = std::collections::BTreeMap::new();

        for (key, value) in snapshot
            .as_ref()
            .find(Some(&prefix), SeekDirection::Forward)
        {
            entries_map.insert(key, value);
        }
        for (key, value) in engine
            .original_snapshot_cache()
            .find(Some(&prefix), SeekDirection::Forward)
        {
            entries_map.entry(key).or_insert(value);
        }

        let entries: Vec<(StorageKey, StorageItem)> = entries_map.into_iter().collect();

        let filtered: Vec<(StorageKey, StorageItem)> = entries
            .into_iter()
            .filter(|(key, _)| {
                let suffix = key.suffix();
                if suffix.len() < 1 + 20 + 20 {
                    return false;
                }
                let account_from_key = UInt160::from_bytes(&suffix[1..21]).ok();
                account_from_key == Some(account)
            })
            .collect();

        let options = FindOptions::RemovePrefix | FindOptions::DeserializeValues;
        let iterator = StorageIterator::new(filtered, 1, options);
        let iterator_id = engine
            .store_storage_iterator(iterator)
            .map_err(CoreError::native_contract)?;

        Ok(iterator_id.to_le_bytes().to_vec())
    }

    fn invoke_create(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() < 7 {
            return Err(CoreError::native_contract(
                "TokenManagement.create: invalid arguments",
            ));
        }

        let token_type = if args[0].is_empty() || args[0][0] == 0 {
            TokenType::Fungible
        } else {
            TokenType::NonFungible
        };

        let owner = UInt160::from_bytes(&args[1])
            .map_err(|_| CoreError::native_contract("Invalid owner"))?;

        let name = String::from_utf8_lossy(&args[2]).to_string();
        let symbol = String::from_utf8_lossy(&args[3]).to_string();

        let decimals = if args[4].is_empty() {
            0
        } else {
            BigInt::from_signed_bytes_le(&args[4])
                .to_u8()
                .ok_or_else(|| CoreError::native_contract("Invalid decimals"))?
        };

        let max_supply = BigInt::from_signed_bytes_le(&args[5]);
        let mintable = !args[6].is_empty() && args[6][0] != 0;

        let asset_id = TokenManagement::get_asset_id(&owner, &name);

        let context = engine.get_native_storage_context(&self.hash())?;

        if self.get_token_state(engine, &asset_id)?.is_some() {
            return Err(CoreError::native_contract(
                "TokenManagement.create: token already exists",
            ));
        }

        let mintable_address = if mintable { Some(owner) } else { None };

        let token_state = TokenState {
            token_type,
            owner,
            name,
            symbol,
            decimals,
            total_supply: BigInt::zero(),
            max_supply,
            mintable_address,
        };

        let key = StorageKey::create_with_uint160(ID, PREFIX_TOKEN_STATE, &asset_id)
            .suffix()
            .to_vec();
        let stack_item = token_state.to_stack_item();
        let bytes = BinarySerializer::serialize(&stack_item, &ExecutionEngineLimits::default())
            .map_err(CoreError::native_contract)?;
        engine.put_storage_item(&context, &key, &bytes)?;

        self.emit_created_event(engine, &asset_id, &token_type)?;

        Ok(asset_id.to_bytes().to_vec())
    }

    fn invoke_create_non_fungible(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() < 4 {
            return Err(CoreError::native_contract(
                "TokenManagement.createNonFungible: invalid arguments",
            ));
        }

        let owner = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid owner"))?;

        let name = String::from_utf8_lossy(&args[1]).to_string();
        let symbol = String::from_utf8_lossy(&args[2]).to_string();

        let mintable = !args[3].is_empty() && args[3][0] != 0;

        let asset_id = TokenManagement::get_asset_id(&owner, &name);

        let context = engine.get_native_storage_context(&self.hash())?;

        if self.get_token_state(engine, &asset_id)?.is_some() {
            return Err(CoreError::native_contract(
                "TokenManagement.createNonFungible: token already exists",
            ));
        }

        let mintable_address = if mintable { Some(owner) } else { None };

        let token_state = TokenState {
            token_type: TokenType::NonFungible,
            owner,
            name,
            symbol,
            decimals: 0,
            total_supply: BigInt::zero(),
            max_supply: BigInt::zero(),
            mintable_address,
        };

        let key = StorageKey::create_with_uint160(ID, PREFIX_TOKEN_STATE, &asset_id)
            .suffix()
            .to_vec();
        let stack_item = token_state.to_stack_item();
        let bytes = BinarySerializer::serialize(&stack_item, &ExecutionEngineLimits::default())
            .map_err(CoreError::native_contract)?;
        engine.put_storage_item(&context, &key, &bytes)?;

        self.emit_created_event(engine, &asset_id, &TokenType::NonFungible)?;

        Ok(asset_id.to_bytes().to_vec())
    }

    fn invoke_mint(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        if args.len() < 2 {
            return Err(CoreError::native_contract(
                "TokenManagement.mint: invalid arguments",
            ));
        }

        let asset_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid asset ID"))?;
        let account = UInt160::from_bytes(&args[1])
            .map_err(|_| CoreError::native_contract("Invalid account"))?;

        let amount = if args.len() > 2 {
            BigInt::from_signed_bytes_le(&args[2])
        } else {
            BigInt::from(1)
        };

        if amount.is_negative() {
            return Err(CoreError::native_contract(
                "TokenManagement.mint: amount cannot be negative",
            ));
        }

        let context = engine.get_native_storage_context(&self.hash())?;

        let Some(mut token_state) = self.get_token_state(engine, &asset_id)? else {
            return Err(CoreError::native_contract(
                "TokenManagement.mint: token not found",
            ));
        };

        if token_state.max_supply > BigInt::zero()
            && token_state.total_supply.clone() + &amount > token_state.max_supply
        {
            return Err(CoreError::native_contract(
                "TokenManagement.mint: max supply exceeded",
            ));
        }

        if let Some(ref mintable_address) = token_state.mintable_address {
            let caller = engine.calling_script_hash();
            if caller != *mintable_address && !engine.check_witness_hash(mintable_address)? {
                return Ok(vec![0]);
            }
        } else {
            return Err(CoreError::native_contract(
                "TokenManagement.mint: token is not mintable",
            ));
        }

        let mut account_state = self
            .get_account_state(engine, &asset_id, &account)?
            .unwrap_or_default();

        account_state.balance += &amount;
        token_state.total_supply += &amount;

        self.write_account_state(&context, engine, &account, &asset_id, &account_state)?;

        let key = StorageKey::create_with_uint160(ID, PREFIX_TOKEN_STATE, &asset_id)
            .suffix()
            .to_vec();
        let stack_item = token_state.to_stack_item();
        let bytes = BinarySerializer::serialize(&stack_item, &ExecutionEngineLimits::default())
            .map_err(CoreError::native_contract)?;
        engine.put_storage_item(&context, &key, &bytes)?;

        self.emit_transfer_event(engine, None, Some(&account), &amount)?;

        Ok(vec![1])
    }

    fn invoke_burn(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        if args.len() < 2 {
            return Err(CoreError::native_contract(
                "TokenManagement.burn: invalid arguments",
            ));
        }

        let asset_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid asset ID"))?;
        let account = UInt160::from_bytes(&args[1])
            .map_err(|_| CoreError::native_contract("Invalid account"))?;

        let amount = if args.len() > 2 {
            BigInt::from_signed_bytes_le(&args[2])
        } else {
            BigInt::from(1)
        };

        if amount.is_negative() {
            return Err(CoreError::native_contract(
                "TokenManagement.burn: amount cannot be negative",
            ));
        }

        let caller = engine.calling_script_hash();
        if caller != account && !engine.check_witness_hash(&account)? {
            return Ok(vec![0]);
        }

        let context = engine.get_native_storage_context(&self.hash())?;

        let Some(mut token_state) = self.get_token_state(engine, &asset_id)? else {
            return Err(CoreError::native_contract(
                "TokenManagement.burn: token not found",
            ));
        };

        let Some(mut account_state) = self.get_account_state(engine, &asset_id, &account)? else {
            return Ok(vec![0]);
        };

        if account_state.balance < amount {
            return Err(CoreError::native_contract(
                "TokenManagement.burn: insufficient balance",
            ));
        }

        account_state.balance -= &amount;
        token_state.total_supply -= &amount;

        if account_state.balance.is_zero() {
            let asset_key = [
                vec![PREFIX_ACCOUNT_STATE],
                account.to_bytes().to_vec(),
                asset_id.to_bytes().to_vec(),
            ]
            .concat();
            engine.delete_storage_item(&context, &asset_key)?;
        } else {
            self.write_account_state(&context, engine, &account, &asset_id, &account_state)?;
        }

        let key = StorageKey::create_with_uint160(ID, PREFIX_TOKEN_STATE, &asset_id)
            .suffix()
            .to_vec();
        let stack_item = token_state.to_stack_item();
        let bytes = BinarySerializer::serialize(&stack_item, &ExecutionEngineLimits::default())
            .map_err(CoreError::native_contract)?;
        engine.put_storage_item(&context, &key, &bytes)?;

        self.emit_transfer_event(engine, Some(&account), None, &amount)?;

        Ok(vec![1])
    }

    fn invoke_transfer(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() < 4 {
            return Err(CoreError::native_contract(
                "TokenManagement.transfer: invalid arguments",
            ));
        }

        let asset_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid asset ID"))?;
        let from = UInt160::from_bytes(&args[1])
            .map_err(|_| CoreError::native_contract("Invalid from address"))?;
        let to = UInt160::from_bytes(&args[2])
            .map_err(|_| CoreError::native_contract("Invalid to address"))?;

        let amount = BigInt::from_signed_bytes_le(&args[3]);

        if amount.is_negative() {
            return Err(CoreError::native_contract(
                "TokenManagement.transfer: amount cannot be negative",
            ));
        }

        if amount.is_zero() {
            return Ok(vec![1]);
        }

        let caller = engine.calling_script_hash();
        if from != caller && !engine.check_witness_hash(&from)? {
            return Ok(vec![0]);
        }

        let context = engine.get_native_storage_context(&self.hash())?;

        let Some(from_state) = self.get_account_state(engine, &asset_id, &from)? else {
            return Ok(vec![0]);
        };

        if from_state.balance < amount {
            return Ok(vec![0]);
        }

        let mut from_balance = from_state.balance;
        from_balance -= &amount;

        let to_state = self
            .get_account_state(engine, &asset_id, &to)?
            .unwrap_or_default();
        let mut to_balance = to_state.balance;
        to_balance += &amount;

        if from_balance.is_zero() {
            let from_key = [
                vec![PREFIX_ACCOUNT_STATE],
                from.to_bytes().to_vec(),
                asset_id.to_bytes().to_vec(),
            ]
            .concat();
            engine.delete_storage_item(&context, &from_key)?;
        } else {
            let from_state = AccountState::with_balance(from_balance);
            self.write_account_state(&context, engine, &from, &asset_id, &from_state)?;
        }

        let to_state = AccountState::with_balance(to_balance);
        self.write_account_state(&context, engine, &to, &asset_id, &to_state)?;

        self.emit_transfer_event(engine, Some(&from), Some(&to), &amount)?;

        Ok(vec![1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_state_default() {
        let state = TokenState::default();
        assert_eq!(state.token_type, TokenType::Fungible);
        assert_eq!(state.total_supply, BigInt::from(0));
    }

    #[test]
    fn test_account_state_new() {
        let state = AccountState::new();
        assert_eq!(state.balance, BigInt::from(0));
    }

    #[test]
    fn test_nft_state_new() {
        let nft = NFTState::new();
        assert!(nft.properties.is_empty());
    }
}
