use std::error::Error;
use std::fmt;

use crate::rpcclient::rolemgmt::{New, NewReader};
use crate::util;
use crate::vm::stackitem::{self, Item};
use crate::core::native::noderoles;
use crate::core::transaction::{self, Transaction};
use crate::crypto::keys::{self, PublicKey};
use crate::neorpc::result;
use crate::util::Uint256;
use crate::util::Uint160;

use anyhow::Result;
use std::sync::Arc;

struct TestAct {
    err: Option<anyhow::Error>,
    res: Option<result::Invoke>,
    tx: Option<Arc<Transaction>>,
    txh: Option<Uint256>,
    vub: Option<u32>,
}

impl TestAct {
    fn new() -> Self {
        TestAct {
            err: None,
            res: None,
            tx: None,
            txh: None,
            vub: None,
        }
    }

    fn call(&self, _contract: Uint160, _operation: &str, _params: &[Item]) -> Result<result::Invoke> {
        match &self.err {
            Some(err) => Err(anyhow::anyhow!(err.to_string())),
            None => Ok(self.res.clone().unwrap()),
        }
    }

    fn make_call(&self, _contract: Uint160, _method: &str, _params: &[Item]) -> Result<Arc<Transaction>> {
        match &self.err {
            Some(err) => Err(anyhow::anyhow!(err.to_string())),
            None => Ok(self.tx.clone().unwrap()),
        }
    }

    fn make_unsigned_call(&self, _contract: Uint160, _method: &str, _attrs: &[transaction::Attribute], _params: &[Item]) -> Result<Arc<Transaction>> {
        match &self.err {
            Some(err) => Err(anyhow::anyhow!(err.to_string())),
            None => Ok(self.tx.clone().unwrap()),
        }
    }

    fn send_call(&self, _contract: Uint160, _method: &str, _params: &[Item]) -> Result<(Uint256, u32)> {
        match &self.err {
            Some(err) => Err(anyhow::anyhow!(err.to_string())),
            None => Ok((self.txh.clone().unwrap(), self.vub.unwrap())),
        }
    }
}

#[test]
fn test_reader_get_designated_by_role() -> Result<()> {
    let ta = TestAct::new();
    let rc = NewReader(ta);

    ta.err = Some(anyhow::anyhow!(""));
    let result = rc.get_designated_by_role(noderoles::Role::Oracle, 0);
    assert!(result.is_err());

    ta.err = None;
    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::from(100500)],
    });
    let result = rc.get_designated_by_role(noderoles::Role::Oracle, 0);
    assert!(result.is_err());

    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::Null],
    });
    let result = rc.get_designated_by_role(noderoles::Role::Oracle, 0);
    assert!(result.is_err());

    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::from(vec![])],
    });
    let nodes = rc.get_designated_by_role(noderoles::Role::Oracle, 0)?;
    assert!(!nodes.is_empty());
    assert_eq!(nodes.len(), 0);

    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::from(vec![stackitem::Item::Null])],
    });
    let result = rc.get_designated_by_role(noderoles::Role::Oracle, 0);
    assert!(result.is_err());

    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::from(vec![stackitem::Item::from(42)])],
    });
    let result = rc.get_designated_by_role(noderoles::Role::Oracle, 0);
    assert!(result.is_err());

    let k = keys::PrivateKey::new()?;
    ta.res = Some(result::Invoke {
        state: "HALT".to_string(),
        stack: vec![stackitem::Item::from(vec![stackitem::Item::from(k.public_key().to_bytes())])],
    });
    let nodes = rc.get_designated_by_role(noderoles::Role::Oracle, 0)?;
    assert!(!nodes.is_empty());
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0], k.public_key());

    Ok(())
}

#[test]
fn test_designate_as_role() -> Result<()> {
    let ta = TestAct::new();
    let rc = New(ta);

    let k = keys::PrivateKey::new()?;
    let ks = vec![k.public_key()];

    ta.err = Some(anyhow::anyhow!(""));
    let result = rc.designate_as_role(noderoles::Role::Oracle, &ks);
    assert!(result.is_err());

    ta.err = None;
    ta.txh = Some(Uint256::from([1, 2, 3]));
    ta.vub = Some(42);
    let (h, vub) = rc.designate_as_role(noderoles::Role::Oracle, &ks)?;
    assert_eq!(ta.txh.unwrap(), h);
    assert_eq!(ta.vub.unwrap(), vub);

    Ok(())
}

#[test]
fn test_designate_as_role_transaction() -> Result<()> {
    let ta = TestAct::new();
    let rc = New(ta);

    let k = keys::PrivateKey::new()?;
    let ks = vec![k.public_key()];

    for fun in &[rc.designate_as_role_transaction, rc.designate_as_role_unsigned] {
        ta.err = Some(anyhow::anyhow!(""));
        let result = fun(noderoles::Role::P2PNotary, &ks);
        assert!(result.is_err());

        ta.err = None;
        ta.tx = Some(Arc::new(Transaction {
            nonce: 100500,
            valid_until_block: 42,
            ..Default::default()
        }));
        let tx = fun(noderoles::Role::P2PNotary, &ks)?;
        assert_eq!(ta.tx.clone().unwrap(), tx);
    }

    Ok(())
}
