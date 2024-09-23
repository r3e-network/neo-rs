use std::error::Error;
use std::sync::Arc;
use std::sync::Mutex;
use uuid::Uuid;
use neo_core2::rpcclient::notary::actor::*;
use neo_core2::rpcclient::invoker::*;
use neo_core2::rpcclient::waiter::*;
use neo_core2::config::netmode;
use neo_core2::core::state;
use neo_core2::core::transaction;
use neo_core2::crypto::keys;
use neo_core2::encoding::address;
use neo_core2::neorpc::result;
use neo_core2::network::payload;
use neo_core2::smartcontract;
use neo_core2::smartcontract::trigger;
use neo_core2::util;
use neo_core2::vm::opcode;
use neo_core2::vm::stackitem;
use neo_core2::vm::vmstate;
use neo_core2::wallet;
use neo_core2::test::require;

struct TestAct {
    err: Option<Box<dyn Error>>,
    res: Option<result::Invoke>,
    tx: Option<transaction::Transaction>,
    txh: util::Uint256,
    vub: u32,
}

impl TestAct {
    fn call(&self, contract: util::Uint160, operation: &str, params: Vec<Box<dyn stackitem::StackItem>>) -> Result<result::Invoke, Box<dyn Error>> {
        match &self.res {
            Some(res) => Ok(res.clone()),
            None => Err(self.err.clone().unwrap_or_else(|| Box::new(std::fmt::Error))),
        }
    }

    fn make_run(&self, script: Vec<u8>) -> Result<transaction::Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap_or_else(|| Box::new(std::fmt::Error))),
        }
    }

    fn make_unsigned_run(&self, script: Vec<u8>, attrs: Vec<transaction::Attribute>) -> Result<transaction::Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap_or_else(|| Box::new(std::fmt::Error))),
        }
    }

    fn send_run(&self, script: Vec<u8>) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        Ok((self.txh, self.vub))
    }

    fn make_call(&self, contract: util::Uint160, method: &str, params: Vec<Box<dyn stackitem::StackItem>>) -> Result<transaction::Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap_or_else(|| Box::new(std::fmt::Error))),
        }
    }

    fn make_unsigned_call(&self, contract: util::Uint160, method: &str, attrs: Vec<transaction::Attribute>, params: Vec<Box<dyn stackitem::StackItem>>) -> Result<transaction::Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap_or_else(|| Box::new(std::fmt::Error))),
        }
    }

    fn send_call(&self, contract: util::Uint160, method: &str, params: Vec<Box<dyn stackitem::StackItem>>) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        Ok((self.txh, self.vub))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use neo_core2::test::require;

    #[test]
    fn test_balance_of() -> Result<()> {
        let mut ta = TestAct {
            err: Some(Box::new(std::fmt::Error)),
            res: None,
            tx: None,
            txh: util::Uint256::default(),
            vub: 0,
        };
        let ntr = NewReader(Arc::new(Mutex::new(ta)));

        ta.err = Some(Box::new(std::fmt::Error));
        let res = ntr.balance_of(util::Uint160::default());
        assert!(res.is_err());

        ta.err = None;
        ta.res = Some(result::Invoke {
            state: "HALT".to_string(),
            stack: vec![stackitem::StackItem::from(42)],
        });
        let res = ntr.balance_of(util::Uint160::default())?;
        assert_eq!(res, 42);

        Ok(())
    }

    #[test]
    fn test_uint32_getters() -> Result<()> {
        let mut ta = TestAct {
            err: Some(Box::new(std::fmt::Error)),
            res: None,
            tx: None,
            txh: util::Uint256::default(),
            vub: 0,
        };
        let ntr = NewReader(Arc::new(Mutex::new(ta)));

        for (name, fun) in vec![
            ("ExpirationOf", || ntr.expiration_of(util::Uint160::default())),
            ("GetMaxNotValidBeforeDelta", || ntr.get_max_not_valid_before_delta()),
        ] {
            ta.err = Some(Box::new(std::fmt::Error));
            let res = fun();
            assert!(res.is_err());

            ta.err = None;
            ta.res = Some(result::Invoke {
                state: "HALT".to_string(),
                stack: vec![stackitem::StackItem::from(42)],
            });
            let res = fun()?;
            assert_eq!(res, 42);

            ta.res = Some(result::Invoke {
                state: "HALT".to_string(),
                stack: vec![stackitem::StackItem::from(-1)],
            });
            let res = fun();
            assert!(res.is_err());
        }

        Ok(())
    }

    #[test]
    fn test_tx_senders() -> Result<()> {
        let mut ta = TestAct {
            err: Some(Box::new(std::fmt::Error)),
            res: None,
            tx: None,
            txh: util::Uint256::default(),
            vub: 0,
        };
        let ntr = New(Arc::new(Mutex::new(ta)));

        for (name, fun) in vec![
            ("LockDepositUntil", || ntr.lock_deposit_until(util::Uint160::default(), 100500)),
            ("SetMaxNotValidBeforeDelta", || ntr.set_max_not_valid_before_delta(42)),
            ("Withdraw", || ntr.withdraw(util::Uint160::default(), util::Uint160::default())),
        ] {
            ta.err = Some(Box::new(std::fmt::Error));
            let res = fun();
            assert!(res.is_err());

            ta.err = None;
            ta.txh = util::Uint256::default();
            ta.vub = 42;
            let (h, vub) = fun()?;
            assert_eq!(h, ta.txh);
            assert_eq!(vub, ta.vub);
        }

        Ok(())
    }

    #[test]
    fn test_tx_makers() -> Result<()> {
        let mut ta = TestAct {
            err: Some(Box::new(std::fmt::Error)),
            res: None,
            tx: None,
            txh: util::Uint256::default(),
            vub: 0,
        };
        let ntr = New(Arc::new(Mutex::new(ta)));

        for (name, fun) in vec![
            ("LockDepositUntilTransaction", || ntr.lock_deposit_until_transaction(util::Uint160::default(), 100500)),
            ("LockDepositUntilUnsigned", || ntr.lock_deposit_until_unsigned(util::Uint160::default(), 100500)),
            ("SetMaxNotValidBeforeDeltaTransaction", || ntr.set_max_not_valid_before_delta_transaction(42)),
            ("SetMaxNotValidBeforeDeltaUnsigned", || ntr.set_max_not_valid_before_delta_unsigned(42)),
            ("WithdrawTransaction", || ntr.withdraw_transaction(util::Uint160::default(), util::Uint160::default())),
            ("WithdrawUnsigned", || ntr.withdraw_unsigned(util::Uint160::default(), util::Uint160::default())),
        ] {
            ta.err = Some(Box::new(std::fmt::Error));
            let res = fun();
            assert!(res.is_err());

            ta.err = None;
            ta.tx = Some(transaction::Transaction {
                nonce: 100500,
                valid_until_block: 42,
                ..Default::default()
            });
            let tx = fun()?;
            assert_eq!(tx, ta.tx.unwrap());
        }

        Ok(())
    }

    #[test]
    fn test_on_nep17_payment_data_convertible() -> Result<()> {
        let d = OnNEP17PaymentData {
            account: Some(util::Uint160::default()),
            till: 123,
        };
        testserdes::to_from_stack_item(&d, &OnNEP17PaymentData::default())?;

        let d = OnNEP17PaymentData {
            account: None,
            till: 123,
        };
        testserdes::to_from_stack_item(&d, &OnNEP17PaymentData::default())?;

        Ok(())
    }

    #[test]
    fn test_on_nep17_payment_data_to_stack_item() -> Result<()> {
        let test_cases = vec![
            (
                "non-empty owner",
                OnNEP17PaymentData {
                    account: Some(util::Uint160::default()),
                    till: 123,
                },
                stackitem::StackItem::Array(vec![
                    stackitem::StackItem::from(util::Uint160::default()),
                    stackitem::StackItem::from(123),
                ]),
            ),
            (
                "empty owner",
                OnNEP17PaymentData {
                    account: None,
                    till: 123,
                },
                stackitem::StackItem::Array(vec![
                    stackitem::StackItem::Null,
                    stackitem::StackItem::from(123),
                ]),
            ),
        ];

        for (name, data, expected) in test_cases {
            let actual = data.to_stack_item()?;
            assert_eq!(actual, expected, "{}", name);
        }

        Ok(())
    }

    #[test]
    fn test_on_nep17_payment_data_from_stack_item() -> Result<()> {
        let err_cases = vec![
            (
                "unexpected stackitem type",
                stackitem::StackItem::Bool(true),
            ),
            (
                "unexpected number of fields",
                stackitem::StackItem::Array(vec![stackitem::StackItem::Bool(true)]),
            ),
            (
                "failed to retrieve account bytes",
                stackitem::StackItem::Array(vec![
                    stackitem::StackItem::Interop(None),
                    stackitem::StackItem::from(1),
                ]),
            ),
            (
                "failed to decode account bytes",
                stackitem::StackItem::Array(vec![
                    stackitem::StackItem::from(vec![1]),
                    stackitem::StackItem::from(1),
                ]),
            ),
            (
                "failed to retrieve till",
                stackitem::StackItem::Array(vec![
                    stackitem::StackItem::from(util::Uint160::default()),
                    stackitem::StackItem::Interop(None),
                ]),
            ),
            (
                "till is not an int64",
                stackitem::StackItem::Array(vec![
                    stackitem::StackItem::from(util::Uint160::default()),
                    stackitem::StackItem::BigInteger(
                        num_bigint::BigInt::from(std::i64::MAX) + 1,
                    ),
                ]),
            ),
            (
                "till is larger than max uint32 value",
                stackitem::StackItem::Array(vec![
                    stackitem::StackItem::from(util::Uint160::default()),
                    stackitem::StackItem::from(std::u32::MAX + 1),
                ]),
            ),
        ];

        for (name, err_case) in err_cases {
            let mut d = OnNEP17PaymentData::default();
            let res = d.from_stack_item(&err_case);
            assert!(res.is_err());
            assert!(res.unwrap_err().to_string().contains(name), "{}", name);
        }

        Ok(())
    }
}
