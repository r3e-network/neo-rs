// Package nns provide RPC wrappers for the non-native NNS contract.
// This is Neo N3 NNS contract wrapper, the source code of the contract can be found here:
// https://github.com/neo-project/non-native-contracts/blob/8d72b92e5e5705d763232bcc24784ced0fb8fc87/src/NameService/NameService.cs

use std::error::Error;
use std::fmt;
use std::str;
use std::sync::Arc;

use bigdecimal::BigDecimal;
use neo_core::transaction::Transaction;
use neo_rpc::result::ApplicationLog;
use neo_rpc::nep11;
use neo_rpc::unwrap;
use neo_smartcontract::smartcontract;
use neo_util::util;
use neo_vm::stackitem::{self, StackItem};

const MAX_NAME_LENGTH: usize = 255;

#[derive(Debug, Clone)]
pub struct SetAdminEvent {
    pub name: String,
    pub old_admin: util::Uint160,
    pub new_admin: util::Uint160,
}

#[derive(Debug, Clone)]
pub struct RenewEvent {
    pub name: String,
    pub old_expiration: BigDecimal,
    pub new_expiration: BigDecimal,
}

pub trait Invoker: nep11::Invoker {}

pub trait Actor: Invoker + nep11::Actor {
    fn make_call(&self, contract: util::Uint160, method: &str, params: Vec<StackItem>) -> Result<Transaction, Box<dyn Error>>;
    fn make_run(&self, script: Vec<u8>) -> Result<Transaction, Box<dyn Error>>;
    fn make_unsigned_call(&self, contract: util::Uint160, method: &str, attrs: Vec<transaction::Attribute>, params: Vec<StackItem>) -> Result<Transaction, Box<dyn Error>>;
    fn make_unsigned_run(&self, script: Vec<u8>, attrs: Vec<transaction::Attribute>) -> Result<Transaction, Box<dyn Error>>;
    fn send_call(&self, contract: util::Uint160, method: &str, params: Vec<StackItem>) -> Result<(util::Uint256, u32), Box<dyn Error>>;
    fn send_run(&self, script: Vec<u8>) -> Result<(util::Uint256, u32), Box<dyn Error>>;
}

pub struct ContractReader {
    invoker: Arc<dyn Invoker>,
    hash: util::Uint160,
}

pub struct Contract {
    reader: ContractReader,
    actor: Arc<dyn Actor>,
    hash: util::Uint160,
}

impl ContractReader {
    pub fn new(invoker: Arc<dyn Invoker>, hash: util::Uint160) -> Self {
        Self { invoker, hash }
    }

    pub fn roots(&self) -> Result<RootIterator, Box<dyn Error>> {
        let (sess, iter) = unwrap::session_iterator(self.invoker.call(self.hash, "roots", vec![]))?;
        Ok(RootIterator { client: self.invoker.clone(), iterator: iter, session: sess })
    }

    pub fn roots_expanded(&self, num_of_iterator_items: usize) -> Result<Vec<String>, Box<dyn Error>> {
        let arr = unwrap::array(self.invoker.call_and_expand_iterator(self.hash, "roots", num_of_iterator_items, vec![]))?;
        items_to_roots(arr)
    }

    pub fn get_price(&self, length: u8) -> Result<BigDecimal, Box<dyn Error>> {
        unwrap::big_int(self.invoker.call(self.hash, "getPrice", vec![StackItem::from(length)]))
    }

    pub fn is_available(&self, name: &str) -> Result<bool, Box<dyn Error>> {
        unwrap::bool(self.invoker.call(self.hash, "isAvailable", vec![StackItem::from(name)]))
    }

    pub fn get_record(&self, name: &str, typev: RecordType) -> Result<String, Box<dyn Error>> {
        unwrap::utf8_string(self.invoker.call(self.hash, "getRecord", vec![StackItem::from(name), StackItem::from(typev as i64)]))
    }

    pub fn get_all_records(&self, name: &str) -> Result<RecordIterator, Box<dyn Error>> {
        let (sess, iter) = unwrap::session_iterator(self.invoker.call(self.hash, "getAllRecords", vec![StackItem::from(name)]))?;
        Ok(RecordIterator { client: self.invoker.clone(), iterator: iter, session: sess })
    }

    pub fn get_all_records_expanded(&self, name: &str, num_of_iterator_items: usize) -> Result<Vec<RecordState>, Box<dyn Error>> {
        let arr = unwrap::array(self.invoker.call_and_expand_iterator(self.hash, "getAllRecords", num_of_iterator_items, vec![StackItem::from(name)]))?;
        items_to_records(arr)
    }

    pub fn resolve(&self, name: &str, typev: RecordType) -> Result<String, Box<dyn Error>> {
        unwrap::utf8_string(self.invoker.call(self.hash, "resolve", vec![StackItem::from(name), StackItem::from(typev as i64)]))
    }
}

impl Contract {
    pub fn new(actor: Arc<dyn Actor>, hash: util::Uint160) -> Self {
        let reader = ContractReader::new(actor.clone(), hash);
        Self { reader, actor, hash }
    }

    pub fn update(&self, nef: Vec<u8>, manifest: &str) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_call(self.hash, "update", vec![StackItem::from(nef), StackItem::from(manifest)])
    }

    pub fn update_transaction(&self, nef: Vec<u8>, manifest: &str) -> Result<Transaction, Box<dyn Error>> {
        self.actor.make_call(self.hash, "update", vec![StackItem::from(nef), StackItem::from(manifest)])
    }

    pub fn update_unsigned(&self, nef: Vec<u8>, manifest: &str) -> Result<Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_call(self.hash, "update", vec![], vec![StackItem::from(nef), StackItem::from(manifest)])
    }

    pub fn add_root(&self, root: &str) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_call(self.hash, "addRoot", vec![StackItem::from(root)])
    }

    pub fn add_root_transaction(&self, root: &str) -> Result<Transaction, Box<dyn Error>> {
        self.actor.make_call(self.hash, "addRoot", vec![StackItem::from(root)])
    }

    pub fn add_root_unsigned(&self, root: &str) -> Result<Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_call(self.hash, "addRoot", vec![], vec![StackItem::from(root)])
    }

    pub fn set_price(&self, price_list: Vec<i64>) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        let any_price_list: Vec<StackItem> = price_list.into_iter().map(StackItem::from).collect();
        self.actor.send_call(self.hash, "setPrice", any_price_list)
    }

    pub fn set_price_transaction(&self, price_list: Vec<i64>) -> Result<Transaction, Box<dyn Error>> {
        let any_price_list: Vec<StackItem> = price_list.into_iter().map(StackItem::from).collect();
        self.actor.make_call(self.hash, "setPrice", any_price_list)
    }

    pub fn set_price_unsigned(&self, price_list: Vec<i64>) -> Result<Transaction, Box<dyn Error>> {
        let any_price_list: Vec<StackItem> = price_list.into_iter().map(StackItem::from).collect();
        self.actor.make_unsigned_call(self.hash, "setPrice", vec![], any_price_list)
    }

    fn script_for_register(&self, name: &str, owner: util::Uint160) -> Result<Vec<u8>, Box<dyn Error>> {
        smartcontract::create_call_with_assert_script(self.hash, "register", vec![StackItem::from(name), StackItem::from(owner)])
    }

    pub fn register(&self, name: &str, owner: util::Uint160) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        let script = self.script_for_register(name, owner)?;
        self.actor.send_run(script)
    }

    pub fn register_transaction(&self, name: &str, owner: util::Uint160) -> Result<Transaction, Box<dyn Error>> {
        let script = self.script_for_register(name, owner)?;
        self.actor.make_run(script)
    }

    pub fn register_unsigned(&self, name: &str, owner: util::Uint160) -> Result<Transaction, Box<dyn Error>> {
        let script = self.script_for_register(name, owner)?;
        self.actor.make_unsigned_run(script, vec![])
    }

    pub fn renew(&self, name: &str) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_call(self.hash, "renew", vec![StackItem::from(name)])
    }

    pub fn renew_transaction(&self, name: &str) -> Result<Transaction, Box<dyn Error>> {
        self.actor.make_call(self.hash, "renew", vec![StackItem::from(name)])
    }

    pub fn renew_unsigned(&self, name: &str) -> Result<Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_call(self.hash, "renew", vec![], vec![StackItem::from(name)])
    }

    pub fn renew2(&self, name: &str, years: i64) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_call(self.hash, "renew", vec![StackItem::from(name), StackItem::from(years)])
    }

    pub fn renew2_transaction(&self, name: &str, years: i64) -> Result<Transaction, Box<dyn Error>> {
        self.actor.make_call(self.hash, "renew", vec![StackItem::from(name), StackItem::from(years)])
    }

    pub fn renew2_unsigned(&self, name: &str, years: i64) -> Result<Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_call(self.hash, "renew", vec![], vec![StackItem::from(name), StackItem::from(years)])
    }

    pub fn set_admin(&self, name: &str, admin: util::Uint160) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_call(self.hash, "setAdmin", vec![StackItem::from(name), StackItem::from(admin)])
    }

    pub fn set_admin_transaction(&self, name: &str, admin: util::Uint160) -> Result<Transaction, Box<dyn Error>> {
        self.actor.make_call(self.hash, "setAdmin", vec![StackItem::from(name), StackItem::from(admin)])
    }

    pub fn set_admin_unsigned(&self, name: &str, admin: util::Uint160) -> Result<Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_call(self.hash, "setAdmin", vec![], vec![StackItem::from(name), StackItem::from(admin)])
    }

    pub fn set_record(&self, name: &str, typev: RecordType, data: &str) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_call(self.hash, "setRecord", vec![StackItem::from(name), StackItem::from(typev as i64), StackItem::from(data)])
    }

    pub fn set_record_transaction(&self, name: &str, typev: RecordType, data: &str) -> Result<Transaction, Box<dyn Error>> {
        self.actor.make_call(self.hash, "setRecord", vec![StackItem::from(name), StackItem::from(typev as i64), StackItem::from(data)])
    }

    pub fn set_record_unsigned(&self, name: &str, typev: RecordType, data: &str) -> Result<Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_call(self.hash, "setRecord", vec![], vec![StackItem::from(name), StackItem::from(typev as i64), StackItem::from(data)])
    }

    pub fn delete_record(&self, name: &str, typev: RecordType) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_call(self.hash, "deleteRecord", vec![StackItem::from(name), StackItem::from(typev as i64)])
    }

    pub fn delete_record_transaction(&self, name: &str, typev: RecordType) -> Result<Transaction, Box<dyn Error>> {
        self.actor.make_call(self.hash, "deleteRecord", vec![StackItem::from(name), StackItem::from(typev as i64)])
    }

    pub fn delete_record_unsigned(&self, name: &str, typev: RecordType) -> Result<Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_call(self.hash, "deleteRecord", vec![], vec![StackItem::from(name), StackItem::from(typev as i64)])
    }
}

impl SetAdminEvent {
    pub fn from_stack_item(item: &StackItem) -> Result<Self, Box<dyn Error>> {
        if let StackItem::Array(arr) = item {
            if arr.len() != 3 {
                return Err(Box::new(fmt::Error::new(fmt::ErrorKind::InvalidInput, "wrong number of structure elements")));
            }

            let name = str::from_utf8(&arr[0].try_bytes()?)?.to_string();
            let old_admin = util::Uint160::decode_bytes_be(&arr[1].try_bytes()?)?;
            let new_admin = util::Uint160::decode_bytes_be(&arr[2].try_bytes()?)?;

            Ok(Self { name, old_admin, new_admin })
        } else {
            Err(Box::new(fmt::Error::new(fmt::ErrorKind::InvalidInput, "not an array")))
        }
    }
}

impl RenewEvent {
    pub fn from_stack_item(item: &StackItem) -> Result<Self, Box<dyn Error>> {
        if let StackItem::Array(arr) = item {
            if arr.len() != 3 {
                return Err(Box::new(fmt::Error::new(fmt::ErrorKind::InvalidInput, "wrong number of structure elements")));
            }

            let name = str::from_utf8(&arr[0].try_bytes()?)?.to_string();
            let old_expiration = BigDecimal::from_str(&arr[1].try_integer()?.to_string())?;
            let new_expiration = BigDecimal::from_str(&arr[2].try_integer()?.to_string())?;

            Ok(Self { name, old_expiration, new_expiration })
        } else {
            Err(Box::new(fmt::Error::new(fmt::ErrorKind::InvalidInput, "not an array")))
        }
    }
}

pub fn set_admin_events_from_application_log(log: &ApplicationLog) -> Result<Vec<SetAdminEvent>, Box<dyn Error>> {
    if log.is_none() {
        return Err(Box::new(fmt::Error::new(fmt::ErrorKind::InvalidInput, "nil application log")));
    }

    let mut res = Vec::new();
    for (i, ex) in log.executions.iter().enumerate() {
        for (j, e) in ex.events.iter().enumerate() {
            if e.name != "SetAdmin" {
                continue;
            }
            let event = SetAdminEvent::from_stack_item(&e.item)?;
            res.push(event);
        }
    }

    Ok(res)
}

pub fn renew_events_from_application_log(log: &ApplicationLog) -> Result<Vec<RenewEvent>, Box<dyn Error>> {
    if log.is_none() {
        return Err(Box::new(fmt::Error::new(fmt::ErrorKind::InvalidInput, "nil application log")));
    }

    let mut res = Vec::new();
    for (i, ex) in log.executions.iter().enumerate() {
        for (j, e) in ex.events.iter().enumerate() {
            if e.name != "Renew" {
                continue;
            }
            let event = RenewEvent::from_stack_item(&e.item)?;
            res.push(event);
        }
    }

    Ok(res)
}
