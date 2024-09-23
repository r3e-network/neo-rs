use std::error::Error;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;
use bigdecimal::BigDecimal;
use neo_core2::rpcclient::nep17::{Nep17Reader, Nep17};
use neo_core2::rpcclient::{Invoker, Actor};
use neo_core2::util::{Uint160, Uint256};
use neo_core2::vm::stackitem;
use neo_core2::core::transaction;
use neo_core2::neorpc::result::Invoke;
use anyhow::Result;

struct TestAct {
    err: Option<Box<dyn Error>>,
    res: Option<Invoke>,
    tx: Option<transaction::Transaction>,
    txh: Uint256,
    vub: u32,
}

impl TestAct {
    fn new() -> Self {
        TestAct {
            err: None,
            res: None,
            tx: None,
            txh: Uint256::default(),
            vub: 0,
        }
    }
}

impl Invoker for TestAct {
    fn call(&self, _contract: Uint160, _operation: &str, _params: &[stackitem::Item]) -> Result<Invoke> {
        if let Some(ref err) = self.err {
            return Err(anyhow::Error::new(err.as_ref()));
        }
        Ok(self.res.clone().unwrap())
    }

    fn make_run(&self, _script: &[u8]) -> Result<transaction::Transaction> {
        if let Some(ref err) = self.err {
            return Err(anyhow::Error::new(err.as_ref()));
        }
        Ok(self.tx.clone().unwrap())
    }

    fn make_unsigned_run(&self, _script: &[u8], _attrs: &[transaction::Attribute]) -> Result<transaction::Transaction> {
        if let Some(ref err) = self.err {
            return Err(anyhow::Error::new(err.as_ref()));
        }
        Ok(self.tx.clone().unwrap())
    }

    fn send_run(&self, _script: &[u8]) -> Result<(Uint256, u32)> {
        if let Some(ref err) = self.err {
            return Err(anyhow::Error::new(err.as_ref()));
        }
        Ok((self.txh, self.vub))
    }
}

#[test]
fn test_reader_balance_of() -> Result<()> {
    let ta = Arc::new(Mutex::new(TestAct::new()));
    let tr = Nep17Reader::new(ta.clone(), Uint160::from([1, 2, 3]));

    {
        let mut ta = ta.lock().unwrap();
        ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
    }
    let res = tr.balance_of(Uint160::from([3, 2, 1]));
    assert!(res.is_err());

    {
        let mut ta = ta.lock().unwrap();
        ta.err = None;
        ta.res = Some(Invoke {
            state: "HALT".to_string(),
            stack: vec![stackitem::Item::from(100500)],
        });
    }
    let bal = tr.balance_of(Uint160::from([3, 2, 1]))?;
    assert_eq!(BigDecimal::from(100500), bal);

    {
        let mut ta = ta.lock().unwrap();
        ta.res = Some(Invoke {
            state: "HALT".to_string(),
            stack: vec![stackitem::Item::from(vec![])],
        });
    }
    let res = tr.balance_of(Uint160::from([3, 2, 1]));
    assert!(res.is_err());

    Ok(())
}

struct TData {
    some_int: i32,
    some_string: String,
}

impl stackitem::StackItemConvertible for TData {
    fn to_stack_item(&self) -> Result<stackitem::Item> {
        Ok(stackitem::Item::Struct(vec![
            stackitem::Item::from(self.some_int),
            stackitem::Item::from(self.some_string.clone()),
        ]))
    }

    fn from_stack_item(_item: stackitem::Item) -> Result<Self> {
        unimplemented!()
    }
}

#[test]
fn test_token_transfer() -> Result<()> {
    let ta = Arc::new(Mutex::new(TestAct::new()));
    let tok = Nep17::new(ta.clone(), Uint160::from([1, 2, 3]));

    let transfer_functions: HashMap<&str, Box<dyn Fn(Uint160, Uint160, BigDecimal, Option<&dyn stackitem::StackItemConvertible>) -> Result<(Uint256, u32)>>> = [
        ("Transfer", Box::new(move |from, to, amount, data| tok.transfer(from, to, amount, data))),
        ("MultiTransfer", Box::new(move |from, to, amount, data| tok.multi_transfer(&[
            (from, to, amount, data),
            (from, to, amount, data),
        ]))),
    ].iter().cloned().collect();

    for (name, fun) in transfer_functions {
        let ta = ta.clone();
        let fun = fun.clone();
        thread::spawn(move || {
            let mut ta = ta.lock().unwrap();
            ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
            let res = fun(Uint160::from([3, 2, 1]), Uint160::from([3, 2, 1]), BigDecimal::from(1), None);
            assert!(res.is_err());

            ta.err = None;
            ta.txh = Uint256::from([1, 2, 3]);
            ta.vub = 42;
            let (h, vub) = fun(Uint160::from([3, 2, 1]), Uint160::from([3, 2, 1]), BigDecimal::from(1), None).unwrap();
            assert_eq!(ta.txh, h);
            assert_eq!(ta.vub, vub);

            let (h, vub) = fun(Uint160::from([3, 2, 1]), Uint160::from([3, 2, 1]), BigDecimal::from(1), Some(&TData {
                some_int: 5,
                some_string: "ur".to_string(),
            })).unwrap();
            assert_eq!(ta.txh, h);
            assert_eq!(ta.vub, vub);

            let res = fun(Uint160::from([3, 2, 1]), Uint160::from([3, 2, 1]), BigDecimal::from(1), Some(&stackitem::Item::Interop(None)));
            assert!(res.is_err());
        }).join().unwrap();
    }

    let res = tok.multi_transfer(&[]);
    assert!(res.is_err());

    Ok(())
}

#[test]
fn test_token_transfer_transaction() -> Result<()> {
    let ta = Arc::new(Mutex::new(TestAct::new()));
    let tok = Nep17::new(ta.clone(), Uint160::from([1, 2, 3]));

    let transfer_functions: HashMap<&str, Box<dyn Fn(Uint160, Uint160, BigDecimal, Option<&dyn stackitem::StackItemConvertible>) -> Result<transaction::Transaction>>> = [
        ("TransferTransaction", Box::new(move |from, to, amount, data| tok.transfer_transaction(from, to, amount, data))),
        ("TransferUnsigned", Box::new(move |from, to, amount, data| tok.transfer_unsigned(from, to, amount, data))),
        ("MultiTransferTransaction", Box::new(move |from, to, amount, data| tok.multi_transfer_transaction(&[
            (from, to, amount, data),
            (from, to, amount, data),
        ]))),
        ("MultiTransferUnsigned", Box::new(move |from, to, amount, data| tok.multi_transfer_unsigned(&[
            (from, to, amount, data),
            (from, to, amount, data),
        ]))),
    ].iter().cloned().collect();

    for (name, fun) in transfer_functions {
        let ta = ta.clone();
        let fun = fun.clone();
        thread::spawn(move || {
            let mut ta = ta.lock().unwrap();
            ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
            let res = fun(Uint160::from([3, 2, 1]), Uint160::from([3, 2, 1]), BigDecimal::from(1), None);
            assert!(res.is_err());

            ta.err = None;
            ta.tx = Some(transaction::Transaction { nonce: 100500, valid_until_block: 42, ..Default::default() });
            let tx = fun(Uint160::from([3, 2, 1]), Uint160::from([3, 2, 1]), BigDecimal::from(1), None).unwrap();
            assert_eq!(ta.tx.as_ref().unwrap(), &tx);

            let tx = fun(Uint160::from([3, 2, 1]), Uint160::from([3, 2, 1]), BigDecimal::from(1), Some(&TData {
                some_int: 5,
                some_string: "ur".to_string(),
            })).unwrap();
            assert_eq!(ta.tx.as_ref().unwrap(), &tx);

            let res = fun(Uint160::from([3, 2, 1]), Uint160::from([3, 2, 1]), BigDecimal::from(1), Some(&stackitem::Item::Interop(None)));
            assert!(res.is_err());
        }).join().unwrap();
    }

    let res = tok.multi_transfer_transaction(&[]);
    assert!(res.is_err());

    let res = tok.multi_transfer_unsigned(&[]);
    assert!(res.is_err());

    Ok(())
}
