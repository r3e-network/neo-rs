use std::error::Error;
use std::sync::Arc;
use std::sync::Mutex;

use crate::core::transaction::{self, Transaction};
use crate::neorpc::result::Invoke;
use crate::util::Uint160;
use crate::util::Uint256;
use crate::transaction::Attribute;
use crate::require;

struct TestAct {
    err: Option<Box<dyn Error>>,
    res: Option<Invoke>,
    tx: Option<Transaction>,
    txh: Option<Uint256>,
    vub: u32,
}

impl TestAct {
    fn call(&self, _contract: Uint160, _operation: &str, _params: &[u8]) -> Result<&Invoke, &Box<dyn Error>> {
        match &self.res {
            Some(res) => Ok(res),
            None => Err(self.err.as_ref().unwrap()),
        }
    }

    fn make_run(&self, _script: &[u8]) -> Result<&Transaction, &Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx),
            None => Err(self.err.as_ref().unwrap()),
        }
    }

    fn make_unsigned_run(&self, _script: &[u8], _attrs: &[Attribute]) -> Result<&Transaction, &Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx),
            None => Err(self.err.as_ref().unwrap()),
        }
    }

    fn send_run(&self, _script: &[u8]) -> Result<(&Uint256, u32), &Box<dyn Error>> {
        match &self.txh {
            Some(txh) => Ok((txh, self.vub)),
            None => Err(self.err.as_ref().unwrap()),
        }
    }
}

#[test]
fn test_new() {
    let ta = TestAct {
        err: None,
        res: None,
        tx: None,
        txh: None,
        vub: 0,
    };
    let gr = NewReader(Arc::new(Mutex::new(ta)));
    assert!(gr.is_some());

    let g = New(Arc::new(Mutex::new(ta)));
    assert!(g.is_some());
}
