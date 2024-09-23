/*
Package nep11 contains RPC wrappers for NEP-11 contracts.

The set of types provided is split between common NEP-11 methods (BaseReader and
Base types) and divisible (DivisibleReader and Divisible) and non-divisible
(NonDivisibleReader and NonDivisible). If you don't know the type of NEP-11
contract you're going to use you can use Base and BaseReader types for many
purposes, otherwise more specific types are recommended.
*/

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use uuid::Uuid;
use bigdecimal::BigDecimal;
use neo_core::transaction::Transaction;
use neo_core::result::{Invoke, Iterator as ResultIterator};
use neo_core::neptoken::{self, Invoker as NeptokenInvoker};
use neo_core::unwrap;
use neo_core::smartcontract;
use neo_core::util::{self, Uint160, Uint256};
use neo_core::vm::stackitem::{self, StackItem};

pub trait Invoker: NeptokenInvoker {
    fn call_and_expand_iterator(&self, contract: Uint160, method: &str, max_items: i32, params: &[&dyn StackItem]) -> Result<Invoke, Box<dyn Error>>;
    fn terminate_session(&self, session_id: Uuid) -> Result<(), Box<dyn Error>>;
    fn traverse_iterator(&self, session_id: Uuid, iterator: &mut ResultIterator, num: i32) -> Result<Vec<Box<dyn StackItem>>, Box<dyn Error>>;
}

pub trait Actor: Invoker {
    fn make_run(&self, script: &[u8]) -> Result<Transaction, Box<dyn Error>>;
    fn make_unsigned_run(&self, script: &[u8], attrs: &[transaction::Attribute]) -> Result<Transaction, Box<dyn Error>>;
    fn send_run(&self, script: &[u8]) -> Result<(Uint256, u32), Box<dyn Error>>;
}

pub struct BaseReader {
    base: neptoken::Base,
    invoker: Arc<dyn Invoker>,
    hash: Uint160,
}

pub struct BaseWriter {
    hash: Uint160,
    actor: Arc<dyn Actor>,
}

pub struct Base {
    reader: BaseReader,
    writer: BaseWriter,
}

pub struct TransferEvent {
    from: Uint160,
    to: Uint160,
    amount: BigDecimal,
    id: Vec<u8>,
}

pub struct TokenIterator {
    client: Arc<dyn Invoker>,
    session: Uuid,
    iterator: ResultIterator,
}

impl BaseReader {
    pub fn new(invoker: Arc<dyn Invoker>, hash: Uint160) -> Self {
        BaseReader {
            base: neptoken::Base::new(invoker.clone(), hash),
            invoker,
            hash,
        }
    }

    pub fn properties(&self, token: &[u8]) -> Result<stackitem::Map, Box<dyn Error>> {
        unwrap::map(self.invoker.call(self.hash, "properties", &[token]))
    }

    pub fn tokens(&self) -> Result<TokenIterator, Box<dyn Error>> {
        let (sess, iter) = unwrap::session_iterator(self.invoker.call(self.hash, "tokens"))?;
        Ok(TokenIterator {
            client: self.invoker.clone(),
            session: sess,
            iterator: iter,
        })
    }

    pub fn tokens_expanded(&self, num: i32) -> Result<Vec<Vec<u8>>, Box<dyn Error>> {
        unwrap::array_of_bytes(self.invoker.call_and_expand_iterator(self.hash, "tokens", num, &[]))
    }

    pub fn tokens_of(&self, account: Uint160) -> Result<TokenIterator, Box<dyn Error>> {
        let (sess, iter) = unwrap::session_iterator(self.invoker.call(self.hash, "tokensOf", &[&account]))?;
        Ok(TokenIterator {
            client: self.invoker.clone(),
            session: sess,
            iterator: iter,
        })
    }

    pub fn tokens_of_expanded(&self, account: Uint160, num: i32) -> Result<Vec<Vec<u8>>, Box<dyn Error>> {
        unwrap::array_of_bytes(self.invoker.call_and_expand_iterator(self.hash, "tokensOf", num, &[&account]))
    }
}

impl BaseWriter {
    pub fn transfer(&self, to: Uint160, id: &[u8], data: &dyn StackItem) -> Result<(Uint256, u32), Box<dyn Error>> {
        let script = self.transfer_script(&[&to, id, data])?;
        self.actor.send_run(&script)
    }

    pub fn transfer_transaction(&self, to: Uint160, id: &[u8], data: &dyn StackItem) -> Result<Transaction, Box<dyn Error>> {
        let script = self.transfer_script(&[&to, id, data])?;
        self.actor.make_run(&script)
    }

    pub fn transfer_unsigned(&self, to: Uint160, id: &[u8], data: &dyn StackItem) -> Result<Transaction, Box<dyn Error>> {
        let script = self.transfer_script(&[&to, id, data])?;
        self.actor.make_unsigned_run(&script, &[])
    }

    fn transfer_script(&self, params: &[&dyn StackItem]) -> Result<Vec<u8>, Box<dyn Error>> {
        smartcontract::create_call_with_assert_script(self.hash, "transfer", params)
    }
}

impl TokenIterator {
    pub fn next(&mut self, num: i32) -> Result<Vec<Vec<u8>>, Box<dyn Error>> {
        let items = self.client.traverse_iterator(self.session, &mut self.iterator, num)?;
        let mut res = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            let b = item.try_bytes().map_err(|e| fmt::format(format_args!("element {} is not a byte string: {}", i, e)))?;
            res.push(b);
        }
        Ok(res)
    }

    pub fn terminate(&self) -> Result<(), Box<dyn Error>> {
        if self.iterator.id.is_none() {
            return Ok(());
        }
        self.client.terminate_session(self.session)
    }
}

pub fn unwrap_known_properties(m: Result<stackitem::Map, Box<dyn Error>>) -> Result<std::collections::HashMap<String, String>, Box<dyn Error>> {
    let m = m?;
    let elems = m.value();
    let mut res = std::collections::HashMap::new();
    for e in elems {
        let k = e.key.try_bytes()?;
        let ks = String::from_utf8(k)?;
        if !result::KNOWN_NEP11_PROPERTIES.contains(&ks) {
            continue;
        }
        let v = e.value.try_bytes()?;
        if !std::str::from_utf8(&v).is_ok() {
            return Err(Box::new(fmt::format(format_args!("invalid {} property: not a UTF-8 string", ks))));
        }
        res.insert(ks, String::from_utf8(v)?);
    }
    Ok(res)
}

pub fn transfer_events_from_application_log(log: &result::ApplicationLog) -> Result<Vec<TransferEvent>, Box<dyn Error>> {
    if log.is_none() {
        return Err(Box::new(fmt::format(format_args!("nil application log"))));
    }
    let mut res = Vec::new();
    for (i, ex) in log.executions.iter().enumerate() {
        for (j, e) in ex.events.iter().enumerate() {
            if e.name != "Transfer" {
                continue;
            }
            let mut event = TransferEvent::default();
            event.from_stack_item(&e.item)?;
            res.push(event);
        }
    }
    Ok(res)
}

impl TransferEvent {
    pub fn from_stack_item(&mut self, item: &stackitem::Array) -> Result<(), Box<dyn Error>> {
        if item.is_none() {
            return Err(Box::new(fmt::format(format_args!("nil item"))));
        }
        let arr = item.value();
        if arr.len() != 4 {
            return Err(Box::new(fmt::format(format_args!("wrong number of event parameters"))));
        }

        let b = arr[0].try_bytes()?;
        self.from = Uint160::decode_bytes_be(&b)?;

        let b = arr[1].try_bytes()?;
        self.to = Uint160::decode_bytes_be(&b)?;

        self.amount = arr[2].try_integer()?;

        let b = arr[3].try_bytes()?;
        self.id = b;

        Ok(())
    }
}
