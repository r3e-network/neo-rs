use std::cmp::Ordering;
use std::convert::TryFrom;
use std::str::FromStr;

use base58::{FromBase58, ToBase58};
use base64::{decode as base64_decode, encode as base64_encode};
use hex::{decode as hex_decode, encode as hex_encode};
use num_bigint::BigInt;
use num_traits::Num;

use crate::core::interop::{Context, ContractMD};
use crate::core::native::nativenames;
use crate::smartcontract::{manifest, CallFlag};
use crate::vm::stackitem::{Item, MaxSize};

const STD_CONTRACT_ID: i32 = -2;
const STD_MAX_INPUT_LENGTH: usize = 1024;

pub struct Std {
    contract_md: ContractMD,
}

impl Std {
    pub fn new() -> Self {
        let mut s = Std {
            contract_md: ContractMD::new(nativenames::STD_LIB, STD_CONTRACT_ID),
        };
        s.build_methods();
        s
    }

    fn build_methods(&mut self) {
        // Add methods here similar to the Go code
        // For example:
        self.add_method("serialize", Self::serialize, 1 << 12, CallFlag::None);
        self.add_method("deserialize", Self::deserialize, 1 << 14, CallFlag::None);
        // ... Add other methods
    }

    fn serialize(&self, ctx: &mut Context, args: Vec<Item>) -> Result<Item, String> {
        let data = ctx.dao.get_item_ctx().serialize(&args[0], false)?;
        if data.len() > MaxSize::try_from(usize::MAX).unwrap() {
            return Err("too big item".into());
        }
        Ok(Item::ByteArray(data.to_vec()))
    }

    fn deserialize(&self, _ctx: &mut Context, args: Vec<Item>) -> Result<Item, String> {
        let data = args[0].try_bytes()?;
        Item::deserialize(&data)
    }

    // Implement other methods here...

    fn to_limited_bytes(&self, item: &Item) -> Result<Vec<u8>, String> {
        let src = item.try_bytes()?;
        if src.len() > STD_MAX_INPUT_LENGTH {
            return Err("input is too big".into());
        }
        Ok(src)
    }

    fn to_limited_string(&self, item: &Item) -> Result<String, String> {
        let src = item.to_string();
        if src.len() > STD_MAX_INPUT_LENGTH {
            return Err("input is too big".into());
        }
        Ok(src)
    }
}

impl Contract for Std {
    fn metadata(&self) -> &ContractMD {
        &self.contract_md
    }

    fn initialize(&mut self, _ctx: &mut Context, _hf: &config::Hardfork) -> Result<(), String> {
        Ok(())
    }

    fn initialize_cache(&mut self, _block_height: u32, _d: &dao::Simple) -> Result<(), String> {
        Ok(())
    }

    fn on_persist(&mut self, _ctx: &mut Context) -> Result<(), String> {
        Ok(())
    }

    fn post_persist(&mut self, _ctx: &mut Context) -> Result<(), String> {
        Ok(())
    }

    fn active_in(&self) -> Option<&config::Hardfork> {
        None
    }
}
