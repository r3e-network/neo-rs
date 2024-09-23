use std::error::Error;
use std::sync::Arc;
use uuid::Uuid;
use bigdecimal::BigDecimal;
use neo_core::state;
use neo_core::transaction;
use neo_core::crypto::keys;
use neo_core::neorpc::result;
use neo_core::util;
use neo_core::vm::stackitem;
use neo_core::require;

struct TestAct {
    err: Option<Box<dyn Error>>,
    res: Option<result::Invoke>,
    tx: Option<transaction::Transaction>,
    txh: util::Uint256,
    vub: u32,
}

impl TestAct {
    fn call(&self, contract: util::Uint160, operation: &str, params: Vec<stackitem::Item>) -> Result<result::Invoke, Box<dyn Error>> {
        match &self.res {
            Some(res) => Ok(res.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn make_run(&self, script: Vec<u8>) -> Result<transaction::Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn make_unsigned_run(&self, script: Vec<u8>, attrs: Vec<transaction::Attribute>) -> Result<transaction::Transaction, Box<dyn Error>> {
        match &self.tx {
            Some(tx) => Ok(tx.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn send_run(&self, script: Vec<u8>) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        Ok((self.txh, self.vub))
    }

    fn call_and_expand_iterator(&self, contract: util::Uint160, method: &str, max_items: i32, params: Vec<stackitem::Item>) -> Result<result::Invoke, Box<dyn Error>> {
        match &self.res {
            Some(res) => Ok(res.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }

    fn terminate_session(&self, session_id: Uuid) -> Result<(), Box<dyn Error>> {
        match &self.err {
            Some(err) => Err(err.clone()),
            None => Ok(()),
        }
    }

    fn traverse_iterator(&self, session_id: Uuid, iterator: &result::Iterator, num: i32) -> Result<Vec<stackitem::Item>, Box<dyn Error>> {
        match &self.res {
            Some(res) => Ok(res.stack.clone()),
            None => Err(self.err.clone().unwrap()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::sync::Arc;
    use uuid::Uuid;
    use bigdecimal::BigDecimal;
    use neo_core::state;
    use neo_core::transaction;
    use neo_core::crypto::keys;
    use neo_core::neorpc::result;
    use neo_core::util;
    use neo_core::vm::stackitem;
    use neo_core::require;

    #[test]
    fn test_reader_balance_of() {
        let mut ta = TestAct {
            err: Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))),
            res: None,
            tx: None,
            txh: util::Uint256::default(),
            vub: 0,
        };
        let tr = NewBaseReader(ta, util::Uint160::from([1, 2, 3]));

        ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
        let err = tr.balance_of(util::Uint160::from([3, 2, 1]));
        assert!(err.is_err());

        ta.err = None;
        ta.res = Some(result::Invoke {
            state: "HALT".to_string(),
            stack: vec![stackitem::Item::from(100500)],
            ..Default::default()
        });
        let bal = tr.balance_of(util::Uint160::from([3, 2, 1])).unwrap();
        assert_eq!(bal, BigDecimal::from(100500));

        ta.res = Some(result::Invoke {
            state: "HALT".to_string(),
            stack: vec![stackitem::Item::Array(vec![])],
            ..Default::default()
        });
        let err = tr.balance_of(util::Uint160::from([3, 2, 1]));
        assert!(err.is_err());
    }

    #[test]
    fn test_reader_properties() {
        let mut ta = TestAct {
            err: Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))),
            res: None,
            tx: None,
            txh: util::Uint256::default(),
            vub: 0,
        };
        let tr = NewBaseReader(ta, util::Uint160::from([1, 2, 3]));

        ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
        let err = tr.properties(vec![3, 2, 1]);
        assert!(err.is_err());

        ta.res = Some(result::Invoke {
            state: "HALT".to_string(),
            stack: vec![stackitem::Item::Array(vec![])],
            ..Default::default()
        });
        let err = tr.properties(vec![3, 2, 1]);
        assert!(err.is_err());

        ta.err = None;
        ta.res = Some(result::Invoke {
            state: "HALT".to_string(),
            stack: vec![stackitem::Item::Map(Default::default())],
            ..Default::default()
        });
        let m = tr.properties(vec![3, 2, 1]).unwrap();
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn test_reader_tokens_of_expanded() {
        let mut ta = TestAct {
            err: Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))),
            res: None,
            tx: None,
            txh: util::Uint256::default(),
            vub: 0,
        };
        let tr = NewBaseReader(ta, util::Uint160::from([1, 2, 3]));

        for (name, fun) in vec![
            ("Tokens", tr.tokens_expanded),
            ("TokensOf", |n| tr.tokens_of_expanded(util::Uint160::from([1, 2, 3]), n)),
        ] {
            ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
            let err = fun(1);
            assert!(err.is_err());

            ta.err = None;
            ta.res = Some(result::Invoke {
                state: "HALT".to_string(),
                stack: vec![stackitem::Item::from(100500)],
                ..Default::default()
            });
            let err = fun(1);
            assert!(err.is_err());

            ta.res = Some(result::Invoke {
                state: "HALT".to_string(),
                stack: vec![stackitem::Item::Array(vec![stackitem::Item::from("one")])],
                ..Default::default()
            });
            let toks = fun(1).unwrap();
            assert_eq!(toks, vec![b"one".to_vec()]);
        }
    }

    #[test]
    fn test_reader_tokens_of() {
        let mut ta = TestAct {
            err: Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))),
            res: None,
            tx: None,
            txh: util::Uint256::default(),
            vub: 0,
        };
        let tr = NewBaseReader(ta, util::Uint160::from([1, 2, 3]));

        for (name, fun) in vec![
            ("Tokens", tr.tokens),
            ("TokensOf", || tr.tokens_of(util::Uint160::from([1, 2, 3]))),
        ] {
            ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
            let err = fun();
            assert!(err.is_err());

            let iid = Uuid::new_v4();
            ta.err = None;
            ta.res = Some(result::Invoke {
                session: Some(Uuid::new_v4()),
                state: "HALT".to_string(),
                stack: vec![stackitem::Item::Interop(result::Iterator {
                    id: Some(iid),
                    ..Default::default()
                })],
                ..Default::default()
            });
            let iter = fun().unwrap();

            ta.res = Some(result::Invoke {
                stack: vec![
                    stackitem::Item::from("one"),
                    stackitem::Item::Array(vec![]),
                ],
                ..Default::default()
            });
            let err = iter.next(10);
            assert!(err.is_err());

            ta.res = Some(result::Invoke {
                stack: vec![
                    stackitem::Item::from("one"),
                    stackitem::Item::from("two"),
                ],
                ..Default::default()
            });
            let vals = iter.next(10).unwrap();
            assert_eq!(vals, vec![b"one".to_vec(), b"two".to_vec()]);

            ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
            let err = iter.next(1);
            assert!(err.is_err());

            let err = iter.terminate();
            assert!(err.is_err());

            // Value-based iterator.
            ta.err = None;
            ta.res = Some(result::Invoke {
                state: "HALT".to_string(),
                stack: vec![stackitem::Item::Interop(result::Iterator {
                    values: vec![
                        stackitem::Item::from("one"),
                        stackitem::Item::from("two"),
                    ],
                    ..Default::default()
                })],
                ..Default::default()
            });
            let iter = fun().unwrap();

            ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
            let err = iter.terminate();
            assert!(err.is_err());
        }
    }

    struct TData {
        some_int: i32,
        some_string: String,
    }

    impl TData {
        fn to_stack_item(&self) -> Result<stackitem::Item, Box<dyn Error>> {
            Ok(stackitem::Item::Struct(vec![
                stackitem::Item::from(self.some_int),
                stackitem::Item::from(self.some_string.clone()),
            ]))
        }

        fn from_stack_item(&self, si: stackitem::Item) -> Result<(), Box<dyn Error>> {
            panic!("TODO")
        }
    }

    #[test]
    fn test_token_transfer() {
        let mut ta = TestAct {
            err: Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))),
            res: None,
            tx: None,
            txh: util::Uint256::default(),
            vub: 0,
        };
        let tok = NewBase(ta, util::Uint160::from([1, 2, 3]));

        ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
        let err = tok.transfer(util::Uint160::from([3, 2, 1]), vec![3, 2, 1], None);
        assert!(err.is_err());

        ta.err = None;
        ta.txh = util::Uint256::from([1, 2, 3]);
        ta.vub = 42;
        let (h, vub) = tok.transfer(util::Uint160::from([3, 2, 1]), vec![3, 2, 1], None).unwrap();
        assert_eq!(h, ta.txh);
        assert_eq!(vub, ta.vub);

        ta.err = None;
        ta.txh = util::Uint256::from([1, 2, 3]);
        ta.vub = 42;
        let (h, vub) = tok.transfer(util::Uint160::from([3, 2, 1]), vec![3, 2, 1], Some(TData {
            some_int: 5,
            some_string: "ur".to_string(),
        })).unwrap();
        assert_eq!(h, ta.txh);
        assert_eq!(vub, ta.vub);

        let err = tok.transfer(util::Uint160::from([3, 2, 1]), vec![3, 2, 1], Some(stackitem::Item::Pointer(123, vec![123])));
        assert!(err.is_err());
    }

    #[test]
    fn test_token_transfer_transaction() {
        let mut ta = TestAct {
            err: Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))),
            res: None,
            tx: None,
            txh: util::Uint256::default(),
            vub: 0,
        };
        let tok = NewBase(ta, util::Uint160::from([1, 2, 3]));

        for fun in vec![
            tok.transfer_transaction,
            tok.transfer_unsigned,
        ] {
            ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
            let err = fun(util::Uint160::from([3, 2, 1]), vec![3, 2, 1], None);
            assert!(err.is_err());

            ta.err = None;
            ta.tx = Some(transaction::Transaction { nonce: 100500, valid_until_block: 42, ..Default::default() });
            let tx = fun(util::Uint160::from([3, 2, 1]), vec![3, 2, 1], None).unwrap();
            assert_eq!(tx, ta.tx.unwrap());

            ta.err = None;
            ta.tx = Some(transaction::Transaction { nonce: 100500, valid_until_block: 42, ..Default::default() });
            let tx = fun(util::Uint160::from([3, 2, 1]), vec![3, 2, 1], Some(TData {
                some_int: 5,
                some_string: "ur".to_string(),
            })).unwrap();
            assert_eq!(tx, ta.tx.unwrap());

            let err = fun(util::Uint160::from([3, 2, 1]), vec![3, 2, 1], Some(stackitem::Item::Interop(None)));
            assert!(err.is_err());
        }
    }

    #[test]
    fn test_unwrap_known_properties() {
        let err = unwrap_known_properties(stackitem::Item::Map(Default::default()), Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, ""))));
        assert!(err.is_err());

        let m = unwrap_known_properties(stackitem::Item::Map(Default::default()), None).unwrap();
        assert!(m.is_some());
        assert_eq!(m.unwrap().len(), 0);

        let m = unwrap_known_properties(stackitem::Item::Map(vec![
            (stackitem::Item::from("some"), stackitem::Item::from("thing")),
        ]), None).unwrap();
        assert!(m.is_some());
        assert_eq!(m.unwrap().len(), 0);

        let m = unwrap_known_properties(stackitem::Item::Map(vec![
            (stackitem::Item::Array(vec![]), stackitem::Item::from("thing")),
        ]), None).unwrap();
        assert!(m.is_some());
        assert_eq!(m.unwrap().len(), 0);

        let err = unwrap_known_properties(stackitem::Item::Map(vec![
            (stackitem::Item::from("name"), stackitem::Item::Array(vec![])),
        ]), None);
        assert!(err.is_err());

        let err = unwrap_known_properties(stackitem::Item::Map(vec![
            (stackitem::Item::from("name"), stackitem::Item::from(vec![0xff])),
        ]), None);
        assert!(err.is_err());

        let m = unwrap_known_properties(stackitem::Item::Map(vec![
            (stackitem::Item::from("name"), stackitem::Item::from("thing")),
            (stackitem::Item::from("description"), stackitem::Item::from("good NFT")),
        ]), None).unwrap();
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m["name"], "thing");
        assert_eq!(m["description"], "good NFT");
    }
}
