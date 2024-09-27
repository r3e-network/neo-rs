use std::collections::HashMap;
use std::sync::Arc;

use neo_core2::core::dao::Simple as DAO;
use neo_core2::core::interop::{Context, ContractMD, MethodAndPrice};
use neo_core2::core::state::StorageItem;
use neo_core2::smartcontract::{manifest, CallFlag};
use neo_core2::util::Uint160;
use num_bigint::BigInt;

const PREFIX_ACCOUNT: u8 = 20;

fn make_account_key(h: &Uint160) -> Vec<u8> {
    make_uint160_key(PREFIX_ACCOUNT, h)
}

struct NEP17TokenNative {
    contract_md: ContractMD,
    symbol: String,
    decimals: i64,
    factor: i64,
    inc_balance: Arc<dyn Fn(&Context, &Uint160, &mut StorageItem, &BigInt, Option<&BigInt>) -> Result<Option<Box<dyn Fn()>>, String>>,
    bal_from_bytes: Arc<dyn Fn(&StorageItem) -> Result<BigInt, String>>,
}

static TOTAL_SUPPLY_KEY: [u8; 1] = [11];

impl NEP17TokenNative {
    fn metadata(&self) -> &ContractMD {
        &self.contract_md
    }
}

fn new_nep17_native(name: String, id: i32) -> NEP17TokenNative {
    let mut n = NEP17TokenNative {
        contract_md: ContractMD::new(name, id, Box::new(|m: &mut manifest::Manifest| {
            m.supported_standards = vec![manifest::NEP17_STANDARD_NAME.to_string()];
        })),
        symbol: String::new(),
        decimals: 0,
        factor: 0,
        inc_balance: Arc::new(|_, _, _, _, _| Ok(None)),
        bal_from_bytes: Arc::new(|_| Ok(BigInt::from(0))),
    };

    let desc = new_descriptor("symbol", "String");
    let md = new_method_and_price(n.symbol, 0, CallFlag::NONE);
    n.contract_md.add_method(md, desc);

    let desc = new_descriptor("decimals", "Integer");
    let md = new_method_and_price(n.decimals, 0, CallFlag::NONE);
    n.contract_md.add_method(md, desc);

    let desc = new_descriptor("totalSupply", "Integer");
    let md = new_method_and_price(n.total_supply, 1 << 15, CallFlag::READ_STATES);
    n.contract_md.add_method(md, desc);

    let desc = new_descriptor("balanceOf", "Integer", vec![manifest::Parameter::new("account", "Hash160")]);
    let md = new_method_and_price(n.balance_of, 1 << 15, CallFlag::READ_STATES);
    n.contract_md.add_method(md, desc);

    let transfer_params = vec![
        manifest::Parameter::new("from", "Hash160"),
        manifest::Parameter::new("to", "Hash160"),
        manifest::Parameter::new("amount", "Integer"),
        manifest::Parameter::new("data", "Any"),
    ];
    let desc = new_descriptor("transfer", "Boolean", transfer_params);
    let mut md = new_method_and_price(n.transfer, 1 << 17, CallFlag::STATES | CallFlag::ALLOW_CALL | CallFlag::ALLOW_NOTIFY);
    md.storage_fee = 50;
    n.contract_md.add_method(md, desc);

    let e_desc = new_event_descriptor("Transfer", &transfer_params[..3]);
    let e_md = new_event(e_desc);
    n.contract_md.add_event(e_md);

    n
}

impl NEP17TokenNative {
    fn initialize(&self, _: &Context) -> Result<(), String> {
        Ok(())
    }

    fn symbol(&self, _: &Context, _: &[stackitem::Item]) -> stackitem::Item {
        stackitem::Item::ByteArray(self.symbol.as_bytes().to_vec())
    }

    fn decimals(&self, _: &Context, _: &[stackitem::Item]) -> stackitem::Item {
        stackitem::Item::Integer(BigInt::from(self.decimals))
    }

    fn total_supply(&self, ic: &Context, _: &[stackitem::Item]) -> stackitem::Item {
        let (_, supply) = self.get_total_supply(&ic.dao);
        stackitem::Item::Integer(supply)
    }

    fn get_total_supply(&self, d: &DAO) -> (StorageItem, BigInt) {
        let si = d.get_storage_item(self.contract_md.id, &TOTAL_SUPPLY_KEY);
        let si = si.unwrap_or_default();
        (si.clone(), BigInt::from_bytes_be(&si))
    }

    fn save_total_supply(&self, d: &mut DAO, _si: StorageItem, supply: &BigInt) {
        d.put_big_int(self.contract_md.id, &TOTAL_SUPPLY_KEY, supply);
    }

    // ... other methods would follow, translated in a similar fashion
}

fn new_descriptor(name: &str, ret: &str, ps: Vec<manifest::Parameter>) -> manifest::Method {
    manifest::Method {
        name: name.to_string(),
        parameters: ps,
        return_type: ret.to_string(),
    }
}

fn new_method_and_price<F>(f: F, cpu_fee: i64, flags: CallFlag) -> MethodAndPrice
where
    F: Fn(&Context, &[stackitem::Item]) -> stackitem::Item + 'static,
{
    MethodAndPrice {
        func: Arc::new(f),
        cpu_fee,
        required_flags: flags,
        active_from: None,
        active_till: None,
    }
}

fn new_event_descriptor(name: &str, ps: &[manifest::Parameter]) -> manifest::Event {
    manifest::Event {
        name: name.to_string(),
        parameters: ps.to_vec(),
    }
}

fn new_event(desc: manifest::Event) -> MethodAndPrice {
    MethodAndPrice {
        func: Arc::new(move |_, _| stackitem::Item::Null),
        cpu_fee: 0,
        required_flags: CallFlag::NONE,
        active_from: None,
        active_till: None,
    }
}

// ... helper functions like to_big_int, to_uint160, etc. would be implemented here
