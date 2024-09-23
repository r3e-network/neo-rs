use std::error::Error;
use std::fmt;

use crate::core::transaction::{self, Transaction};
use crate::neorpc::result;
use crate::util;
use crate::vm::stackitem;
use anyhow::Result;
use util::Uint256;
use util::Uint160;

struct TestAct {
    err: Option<Box<dyn Error>>,
    res: Option<result::Invoke>,
    tx: Option<Transaction>,
    txh: Uint256,
    vub: u32,
}

impl TestAct {
    fn call(&self, _contract: Uint160, _operation: &str, _params: &[impl fmt::Debug]) -> Result<&result::Invoke> {
        match &self.res {
            Some(res) => Ok(res),
            None => Err(anyhow::anyhow!("Error: {:?}", self.err)),
        }
    }

    fn make_call(&self, _contract: Uint160, _method: &str, _params: &[impl fmt::Debug]) -> Result<&Transaction> {
        match &self.tx {
            Some(tx) => Ok(tx),
            None => Err(anyhow::anyhow!("Error: {:?}", self.err)),
        }
    }

    fn make_unsigned_call(&self, _contract: Uint160, _method: &str, _attrs: &[transaction::Attribute], _params: &[impl fmt::Debug]) -> Result<&Transaction> {
        match &self.tx {
            Some(tx) => Ok(tx),
            None => Err(anyhow::anyhow!("Error: {:?}", self.err)),
        }
    }

    fn send_call(&self, _contract: Uint160, _method: &str, _params: &[impl fmt::Debug]) -> Result<(Uint256, u32)> {
        if self.err.is_some() {
            Err(anyhow::anyhow!("Error: {:?}", self.err))
        } else {
            Ok((self.txh, self.vub))
        }
    }

    fn make_run(&self, _script: &[u8]) -> Result<&Transaction> {
        match &self.tx {
            Some(tx) => Ok(tx),
            None => Err(anyhow::anyhow!("Error: {:?}", self.err)),
        }
    }

    fn make_unsigned_run(&self, _script: &[u8], _attrs: &[transaction::Attribute]) -> Result<&Transaction> {
        match &self.tx {
            Some(tx) => Ok(tx),
            None => Err(anyhow::anyhow!("Error: {:?}", self.err)),
        }
    }

    fn send_run(&self, _script: &[u8]) -> Result<(Uint256, u32)> {
        if self.err.is_some() {
            Err(anyhow::anyhow!("Error: {:?}", self.err))
        } else {
            Ok((self.txh, self.vub))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::transaction::ConflictsT;
    use crate::util::Uint160;
    use crate::vm::stackitem;
    use anyhow::anyhow;
    use std::error::Error;

    #[test]
    fn test_reader() {
        let mut ta = TestAct {
            err: Some(Box::new(anyhow!(""))),
            res: None,
            tx: None,
            txh: Uint256::default(),
            vub: 0,
        };
        let pc = NewReader(&ta);

        let meth: Vec<fn() -> Result<i64>> = vec![
            pc.get_exec_fee_factor,
            pc.get_fee_per_byte,
            pc.get_storage_price,
        ];

        for m in &meth {
            let err = m().unwrap_err();
            assert!(err.is::<anyhow::Error>());
        }

        let err = pc.is_blocked(Uint160::from([1, 2, 3])).unwrap_err();
        assert!(err.is::<anyhow::Error>());

        let err = pc.get_attribute_fee(ConflictsT).unwrap_err();
        assert!(err.is::<anyhow::Error>());

        ta.err = None;
        ta.res = Some(result::Invoke {
            state: "HALT".to_string(),
            stack: vec![stackitem::Item::from(42)],
        });

        for m in &meth {
            let val = m().unwrap();
            assert_eq!(val, 42);
        }

        let v = pc.get_attribute_fee(ConflictsT).unwrap();
        assert_eq!(v, 42);

        ta.res = Some(result::Invoke {
            state: "HALT".to_string(),
            stack: vec![stackitem::Item::from(true)],
        });

        let val = pc.is_blocked(Uint160::from([1, 2, 3])).unwrap();
        assert!(val);
    }

    #[test]
    fn test_int_setters() {
        let mut ta = TestAct {
            err: Some(Box::new(anyhow!(""))),
            res: None,
            tx: None,
            txh: Uint256::default(),
            vub: 0,
        };
        let pc = New(&ta);

        let meth: Vec<fn(i64) -> Result<(Uint256, u32)>> = vec![
            pc.set_exec_fee_factor,
            pc.set_fee_per_byte,
            pc.set_storage_price,
        ];

        for m in &meth {
            let err = m(42).unwrap_err();
            assert!(err.is::<anyhow::Error>());
        }

        let err = pc.set_attribute_fee(transaction::OracleResponseT, 123).unwrap_err();
        assert!(err.is::<anyhow::Error>());

        ta.err = None;
        ta.txh = Uint256::from([1, 2, 3]);
        ta.vub = 42;

        for m in &meth {
            let (h, vub) = m(100).unwrap();
            assert_eq!(h, ta.txh);
            assert_eq!(vub, ta.vub);
        }

        let (h, vub) = pc.set_attribute_fee(transaction::OracleResponseT, 123).unwrap();
        assert_eq!(h, ta.txh);
        assert_eq!(vub, ta.vub);
    }

    #[test]
    fn test_uint160_setters() {
        let mut ta = TestAct {
            err: Some(Box::new(anyhow!(""))),
            res: None,
            tx: None,
            txh: Uint256::default(),
            vub: 0,
        };
        let pc = New(&ta);

        let meth: Vec<fn(Uint160) -> Result<(Uint256, u32)>> = vec![
            pc.block_account,
            pc.unblock_account,
        ];

        for m in &meth {
            let err = m(Uint160::default()).unwrap_err();
            assert!(err.is::<anyhow::Error>());
        }

        ta.err = None;
        ta.txh = Uint256::from([1, 2, 3]);
        ta.vub = 42;

        for m in &meth {
            let (h, vub) = m(Uint160::default()).unwrap();
            assert_eq!(h, ta.txh);
            assert_eq!(vub, ta.vub);
        }
    }

    #[test]
    fn test_int_transactions() {
        let mut ta = TestAct {
            err: Some(Box::new(anyhow!(""))),
            res: None,
            tx: Some(Transaction {
                nonce: 100500,
                valid_until_block: 42,
                ..Default::default()
            }),
            txh: Uint256::default(),
            vub: 0,
        };
        let pc = New(&ta);

        let funcs: Vec<fn(i64) -> Result<&Transaction>> = vec![
            pc.set_exec_fee_factor_transaction,
            pc.set_exec_fee_factor_unsigned,
            pc.set_fee_per_byte_transaction,
            pc.set_fee_per_byte_unsigned,
            pc.set_storage_price_transaction,
            pc.set_storage_price_unsigned,
        ];

        for fun in &funcs {
            ta.err = Some(Box::new(anyhow!("")));
            let err = fun(1).unwrap_err();
            assert!(err.is::<anyhow::Error>());

            ta.err = None;
            let tx = fun(1).unwrap();
            assert_eq!(tx.nonce, 100500);
            assert_eq!(tx.valid_until_block, 42);
        }
    }

    #[test]
    fn test_uint160_transactions() {
        let mut ta = TestAct {
            err: Some(Box::new(anyhow!(""))),
            res: None,
            tx: Some(Transaction {
                nonce: 100500,
                valid_until_block: 42,
                ..Default::default()
            }),
            txh: Uint256::default(),
            vub: 0,
        };
        let pc = New(&ta);

        let funcs: Vec<fn(Uint160) -> Result<&Transaction>> = vec![
            pc.block_account_transaction,
            pc.block_account_unsigned,
            pc.unblock_account_transaction,
            pc.unblock_account_unsigned,
        ];

        for fun in &funcs {
            ta.err = Some(Box::new(anyhow!("")));
            let err = fun(Uint160::from([1])).unwrap_err();
            assert!(err.is::<anyhow::Error>());

            ta.err = None;
            let tx = fun(Uint160::from([1])).unwrap();
            assert_eq!(tx.nonce, 100500);
            assert_eq!(tx.valid_until_block, 42);
        }
    }
}
