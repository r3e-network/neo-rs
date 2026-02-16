use crate::UInt160;
use crate::error::CoreError;
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::stack_item_extract::{extract_bytes, extract_int, extract_u8};
use neo_vm::StackItem;
use num_bigint::BigInt;

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

impl IInteroperable for TokenState {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), CoreError> {
        if let StackItem::Struct(struct_item) = stack_item {
            let items = struct_item.items();
            if items.len() >= 7 {
                if let Some(token_type_int) = extract_int(&items[0]) {
                    self.token_type = if token_type_int == BigInt::from(1) {
                        TokenType::NonFungible
                    } else {
                        TokenType::Fungible
                    };
                }
                if let Some(bytes) = extract_bytes(&items[1])
                    && let Ok(owner) = UInt160::from_bytes(&bytes)
                {
                    self.owner = owner;
                }
                if let Some(bytes) = extract_bytes(&items[2]) {
                    self.name = String::from_utf8_lossy(&bytes).to_string();
                }
                if let Some(bytes) = extract_bytes(&items[3]) {
                    self.symbol = String::from_utf8_lossy(&bytes).to_string();
                }
                if let Some(decimals) = extract_u8(&items[4]) {
                    self.decimals = decimals;
                }
                if let Some(total_supply) = extract_int(&items[5]) {
                    self.total_supply = total_supply;
                }
                if let Some(max_supply) = extract_int(&items[6]) {
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
        Ok(())
    }

    fn to_stack_item(&self) -> Result<StackItem, CoreError> {
        let items = vec![
            StackItem::from_int(self.token_type as i32),
            StackItem::from_byte_string(self.owner.to_bytes()),
            StackItem::from_byte_string(self.name.as_bytes()),
            StackItem::from_byte_string(self.symbol.as_bytes()),
            StackItem::from_int(self.decimals as i32),
            StackItem::from_int(self.total_supply.clone()),
            StackItem::from_int(self.max_supply.clone()),
            StackItem::from_bool(self.mintable_address.is_some()),
        ];
        Ok(StackItem::from_struct(items))
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
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), CoreError> {
        if let StackItem::Struct(struct_item) = stack_item {
            let items = struct_item.items();
            if items.len() >= 2 {
                if let Some(bytes) = extract_bytes(&items[0])
                    && let Ok(asset_id) = UInt160::from_bytes(&bytes)
                {
                    self.asset_id = asset_id;
                }
                if let Some(bytes) = extract_bytes(&items[1])
                    && let Ok(owner) = UInt160::from_bytes(&bytes)
                {
                    self.owner = owner;
                }
                if items.len() >= 3 {
                    if let Ok(properties_array) = items[2].as_array() {
                        self.properties = properties_array
                            .iter()
                            .filter_map(|prop| {
                                if let StackItem::Struct(prop_struct) = prop {
                                    let prop_items = prop_struct.items();
                                    if prop_items.len() >= 2 {
                                        let key = extract_bytes(&prop_items[0])?;
                                        let value = extract_bytes(&prop_items[1])?;
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

    fn to_stack_item(&self) -> Result<StackItem, CoreError> {
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
        Ok(StackItem::from_struct(vec![
            StackItem::from_byte_string(self.asset_id.to_bytes()),
            StackItem::from_byte_string(self.owner.to_bytes()),
            StackItem::from_array(properties_items),
        ]))
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}
