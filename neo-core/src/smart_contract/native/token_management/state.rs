use super::stack_value::{
    bigint_stack_value, stack_item_to_stack_value, stack_value_to_bigint, stack_value_to_bool,
    stack_value_to_bytes, stack_value_to_stack_item,
};
use crate::error::CoreError;
use crate::smart_contract::interoperable::Interoperable;
use crate::vm_runtime::StackItem;
use crate::UInt160;
use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// Type of token (fungible or non-fungible).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    /// Fungible token (divisible, interchangeable).
    Fungible = 0,
    /// Non-fungible token (unique, indivisible).
    NonFungible = 1,
}

/// State of a registered token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenState {
    /// Type of the token.
    pub token_type: TokenType,
    /// Owner address of the token contract.
    pub owner: UInt160,
    /// Human-readable name of the token.
    pub name: String,
    /// Short symbol for the token.
    pub symbol: String,
    /// Number of decimal places.
    pub decimals: u8,
    /// Current total supply.
    pub total_supply: BigInt,
    /// Maximum allowed supply.
    pub max_supply: BigInt,
    /// Address allowed to mint new tokens.
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

impl Interoperable for TokenState {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), CoreError> {
        self.from_stack_value(stack_item_to_stack_value(stack_item, "TokenState")?)
    }

    fn to_stack_item(&self) -> Result<StackItem, CoreError> {
        stack_value_to_stack_item(self.to_stack_value(), "TokenState")
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

impl TokenState {
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        if let StackValue::Struct(items) = stack_value {
            if items.len() >= 7 {
                if let Ok(token_type_int) = stack_value_to_bigint(&items[0]) {
                    self.token_type = if token_type_int == BigInt::from(1) {
                        TokenType::NonFungible
                    } else {
                        TokenType::Fungible
                    };
                }
                if let Ok(bytes) = stack_value_to_bytes(&items[1]) {
                    if let Ok(owner) = UInt160::from_bytes(&bytes) {
                        self.owner = owner;
                    }
                }
                if let Ok(bytes) = stack_value_to_bytes(&items[2]) {
                    self.name = String::from_utf8_lossy(&bytes).to_string();
                }
                if let Ok(bytes) = stack_value_to_bytes(&items[3]) {
                    self.symbol = String::from_utf8_lossy(&bytes).to_string();
                }
                if let Ok(decimals_int) = stack_value_to_bigint(&items[4]) {
                    self.decimals = decimals_int.to_u8().unwrap_or(0);
                }
                if let Ok(total_supply) = stack_value_to_bigint(&items[5]) {
                    self.total_supply = total_supply;
                }
                if let Ok(max_supply) = stack_value_to_bigint(&items[6]) {
                    self.max_supply = max_supply;
                }
                if items.len() >= 8 {
                    if let Ok(mintable) = stack_value_to_bool(&items[7]) {
                        if mintable {
                            self.mintable_address = Some(self.owner);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(vec![
            StackValue::Integer(self.token_type as i64),
            StackValue::ByteString(self.owner.to_bytes()),
            StackValue::ByteString(self.name.as_bytes().to_vec()),
            StackValue::ByteString(self.symbol.as_bytes().to_vec()),
            StackValue::Integer(i64::from(self.decimals)),
            bigint_stack_value(&self.total_supply),
            bigint_stack_value(&self.max_supply),
            StackValue::Boolean(self.mintable_address.is_some()),
        ])
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

impl Interoperable for AccountState {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), CoreError> {
        self.from_stack_value(stack_item_to_stack_value(stack_item, "AccountState")?)
    }

    fn to_stack_item(&self) -> Result<StackItem, CoreError> {
        stack_value_to_stack_item(self.to_stack_value(), "AccountState")
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

impl AccountState {
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        if let StackValue::Struct(items) = stack_value {
            if let Some(first) = items.first() {
                if let Ok(integer) = stack_value_to_bigint(first) {
                    self.balance = integer;
                }
            }
        }
        Ok(())
    }

    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(vec![bigint_stack_value(&self.balance)])
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

impl Interoperable for NFTState {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), CoreError> {
        self.from_stack_value(stack_item_to_stack_value(stack_item, "NFTState")?)
    }

    fn to_stack_item(&self) -> Result<StackItem, CoreError> {
        stack_value_to_stack_item(self.to_stack_value(), "NFTState")
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

impl NFTState {
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        if let StackValue::Struct(items) = stack_value {
            if items.len() >= 2 {
                if let Ok(bytes) = stack_value_to_bytes(&items[0]) {
                    if let Ok(asset_id) = UInt160::from_bytes(&bytes) {
                        self.asset_id = asset_id;
                    }
                }
                if let Ok(bytes) = stack_value_to_bytes(&items[1]) {
                    if let Ok(owner) = UInt160::from_bytes(&bytes) {
                        self.owner = owner;
                    }
                }
                if items.len() >= 3 {
                    if let StackValue::Array(properties_array)
                    | StackValue::Struct(properties_array) = &items[2]
                    {
                        self.properties = properties_array
                            .iter()
                            .filter_map(|prop| {
                                if let StackValue::Struct(prop_items) = prop {
                                    if prop_items.len() >= 2 {
                                        let key = stack_value_to_bytes(&prop_items[0]).ok()?;
                                        let value = stack_value_to_bytes(&prop_items[1]).ok()?;
                                        Some((key, value))
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            })
                            .collect();
                    }
                }
            }
        }
        Ok(())
    }

    pub fn to_stack_value(&self) -> StackValue {
        let properties_items: Vec<StackValue> = self
            .properties
            .iter()
            .map(|(k, v)| {
                StackValue::Struct(vec![
                    StackValue::ByteString(k.clone()),
                    StackValue::ByteString(v.clone()),
                ])
            })
            .collect();
        StackValue::Struct(vec![
            StackValue::ByteString(self.asset_id.to_bytes()),
            StackValue::ByteString(self.owner.to_bytes()),
            StackValue::Array(properties_items),
        ])
    }
}
