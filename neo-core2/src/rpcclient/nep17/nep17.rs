/*
Package nep17 contains RPC wrappers to work with NEP-17 contracts.

Safe methods are encapsulated into TokenReader structure while Token provides
various methods to perform the only NEP-17 state-changing call, Transfer.
*/

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use bigdecimal::BigDecimal;
use crate::core::transaction::{Transaction, Attribute};
use crate::neorpc::result::ApplicationLog;
use crate::rpcclient::neptoken::{self, Invoker as NeptokenInvoker};
use crate::smartcontract::Builder;
use crate::util::{self, Uint160, Uint256};
use crate::vm::stackitem::{self, StackItem};

pub trait Invoker: NeptokenInvoker {}

pub trait Actor: Invoker {
    fn make_run(&self, script: &[u8]) -> Result<Transaction, Box<dyn Error>>;
    fn make_unsigned_run(&self, script: &[u8], attrs: &[Attribute]) -> Result<Transaction, Box<dyn Error>>;
    fn send_run(&self, script: &[u8]) -> Result<(Uint256, u32), Box<dyn Error>>;
}

pub struct TokenReader {
    base: neptoken::Base,
}

pub struct TokenWriter {
    hash: Uint160,
    actor: Arc<dyn Actor>,
}

pub struct Token {
    reader: TokenReader,
    writer: TokenWriter,
}

pub struct TransferEvent {
    from: Uint160,
    to: Uint160,
    amount: BigDecimal,
}

pub struct TransferParameters {
    from: Uint160,
    to: Uint160,
    amount: BigDecimal,
    data: Box<dyn std::any::Any>,
}

impl TokenReader {
    pub fn new(invoker: Arc<dyn Invoker>, hash: Uint160) -> Self {
        TokenReader {
            base: neptoken::Base::new(invoker, hash),
        }
    }
}

impl Token {
    pub fn new(actor: Arc<dyn Actor>, hash: Uint160) -> Self {
        Token {
            reader: TokenReader::new(actor.clone(), hash),
            writer: TokenWriter { hash, actor },
        }
    }
}

impl TokenWriter {
    pub fn transfer(&self, from: Uint160, to: Uint160, amount: BigDecimal, data: Box<dyn std::any::Any>) -> Result<(Uint256, u32), Box<dyn Error>> {
        self.multi_transfer(vec![TransferParameters { from, to, amount, data }])
    }

    pub fn transfer_transaction(&self, from: Uint160, to: Uint160, amount: BigDecimal, data: Box<dyn std::any::Any>) -> Result<Transaction, Box<dyn Error>> {
        self.multi_transfer_transaction(vec![TransferParameters { from, to, amount, data }])
    }

    pub fn transfer_unsigned(&self, from: Uint160, to: Uint160, amount: BigDecimal, data: Box<dyn std::any::Any>) -> Result<Transaction, Box<dyn Error>> {
        self.multi_transfer_unsigned(vec![TransferParameters { from, to, amount, data }])
    }

    fn multi_transfer_script(&self, params: Vec<TransferParameters>) -> Result<Vec<u8>, Box<dyn Error>> {
        if params.is_empty() {
            return Err(Box::new(fmt::Error::new(fmt::Error, "at least one transfer parameter required")));
        }
        let mut b = Builder::new();
        for param in params {
            b.invoke_with_assert(self.hash, "transfer", param.from, param.to, param.amount, param.data);
        }
        Ok(b.script())
    }

    pub fn multi_transfer(&self, params: Vec<TransferParameters>) -> Result<(Uint256, u32), Box<dyn Error>> {
        let script = self.multi_transfer_script(params)?;
        self.actor.send_run(&script)
    }

    pub fn multi_transfer_transaction(&self, params: Vec<TransferParameters>) -> Result<Transaction, Box<dyn Error>> {
        let script = self.multi_transfer_script(params)?;
        self.actor.make_run(&script)
    }

    pub fn multi_transfer_unsigned(&self, params: Vec<TransferParameters>) -> Result<Transaction, Box<dyn Error>> {
        let script = self.multi_transfer_script(params)?;
        self.actor.make_unsigned_run(&script, &[])
    }
}

impl TransferEvent {
    pub fn from_application_log(log: &ApplicationLog) -> Result<Vec<TransferEvent>, Box<dyn Error>> {
        if log.is_none() {
            return Err(Box::new(fmt::Error::new(fmt::Error, "nil application log")));
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

    pub fn from_stack_item(&mut self, item: &StackItem) -> Result<(), Box<dyn Error>> {
        if item.is_none() {
            return Err(Box::new(fmt::Error::new(fmt::Error, "nil item")));
        }
        let arr = item.value().as_array().ok_or_else(|| fmt::Error::new(fmt::Error, "not an array"))?;
        if arr.len() != 3 {
            return Err(Box::new(fmt::Error::new(fmt::Error, "wrong number of event parameters")));
        }

        let b = arr[0].try_bytes()?;
        self.from = Uint160::decode_bytes_be(&b)?;

        let b = arr[1].try_bytes()?;
        self.to = Uint160::decode_bytes_be(&b)?;

        self.amount = arr[2].try_integer()?;

        Ok(())
    }
}
