use std::error::Error;
use std::sync::Arc;
use std::sync::Mutex;
use std::collections::HashMap;
use uuid::Uuid;
use num_bigint::BigInt;
use neo_core::transaction::Transaction;
use neo_core::neorpc::result::Invoke;
use neo_core::util::Uint160;
use neo_core::vm::stackitem::StackItem;
use neo_core::test_utils::require;

struct TestAct {
    err: Option<Box<dyn Error>>,
    res: Option<Invoke>,
    txh: Option<Uint256>,
    vub: Option<u32>,
    tx: Option<Transaction>,
}

impl TestAct {
    fn new() -> Self {
        Self {
            err: None,
            res: None,
            txh: None,
            vub: None,
            tx: None,
        }
    }
}

#[test]
fn test_divisible_balance_of() {
    let ta = Arc::new(Mutex::new(TestAct::new()));
    let tr = NewDivisibleReader(ta.clone(), Uint160::new([1, 2, 3]));
    let tt = NewDivisible(ta.clone(), Uint160::new([1, 2, 3]));

    let tests: HashMap<&str, Box<dyn Fn(Uint160, &[u8]) -> Result<BigInt, Box<dyn Error>>>> = [
        ("Reader", Box::new(move |a, b| tr.balance_of_d(a, b))),
        ("Full", Box::new(move |a, b| tt.balance_of_d(a, b))),
    ].iter().cloned().collect();

    for (name, fun) in tests {
        ta.lock().unwrap().err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
        assert!(fun(Uint160::new([3, 2, 1]), &[1, 2, 3]).is_err());

        ta.lock().unwrap().err = None;
        ta.lock().unwrap().res = Some(Invoke {
            state: "HALT".to_string(),
            stack: vec![StackItem::from(100500)],
        });
        let bal = fun(Uint160::new([3, 2, 1]), &[1, 2, 3]).unwrap();
        assert_eq!(bal, BigInt::from(100500));

        ta.lock().unwrap().res = Some(Invoke {
            state: "HALT".to_string(),
            stack: vec![StackItem::from(vec![])],
        });
        assert!(fun(Uint160::new([3, 2, 1]), &[1, 2, 3]).is_err());
    }
}

#[test]
fn test_divisible_owner_of_expanded() {
    let ta = Arc::new(Mutex::new(TestAct::new()));
    let tr = NewDivisibleReader(ta.clone(), Uint160::new([1, 2, 3]));
    let tt = NewDivisible(ta.clone(), Uint160::new([1, 2, 3]));

    let tests: HashMap<&str, Box<dyn Fn(&[u8], i32) -> Result<Vec<Uint160>, Box<dyn Error>>>> = [
        ("Reader", Box::new(move |a, b| tr.owner_of_expanded(a, b))),
        ("Full", Box::new(move |a, b| tt.owner_of_expanded(a, b))),
    ].iter().cloned().collect();

    for (name, fun) in tests {
        ta.lock().unwrap().err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
        assert!(fun(&[1, 2, 3], 1).is_err());

        ta.lock().unwrap().err = None;
        ta.lock().unwrap().res = Some(Invoke {
            state: "HALT".to_string(),
            stack: vec![StackItem::from(100500)],
        });
        assert!(fun(&[1, 2, 3], 1).is_err());

        let h = Uint160::new([3, 2, 1]);
        ta.lock().unwrap().res = Some(Invoke {
            state: "HALT".to_string(),
            stack: vec![StackItem::from(vec![StackItem::from(h.to_bytes_be())])],
        });
        let owls = fun(&[1, 2, 3], 1).unwrap();
        assert_eq!(owls, vec![h]);
    }
}

#[test]
fn test_divisible_owner_of() {
    let ta = Arc::new(Mutex::new(TestAct::new()));
    let tr = NewDivisibleReader(ta.clone(), Uint160::new([1, 2, 3]));
    let tt = NewDivisible(ta.clone(), Uint160::new([1, 2, 3]));

    let tests: HashMap<&str, Box<dyn Fn(&[u8]) -> Result<OwnerIterator, Box<dyn Error>>>> = [
        ("Reader", Box::new(move |a| tr.owner_of(a))),
        ("Full", Box::new(move |a| tt.owner_of(a))),
    ].iter().cloned().collect();

    for (name, fun) in tests {
        ta.lock().unwrap().err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
        assert!(fun(&[1]).is_err());

        let iid = Uuid::new_v4();
        ta.lock().unwrap().err = None;
        ta.lock().unwrap().res = Some(Invoke {
            session: Some(Uuid::new_v4()),
            state: "HALT".to_string(),
            stack: vec![StackItem::new_interop(result::Iterator {
                id: Some(iid),
            })],
        });
        let mut iter = fun(&[1]).unwrap();

        ta.lock().unwrap().res = Some(Invoke {
            stack: vec![StackItem::from(vec![])],
        });
        assert!(iter.next(10).is_err());

        ta.lock().unwrap().res = Some(Invoke {
            stack: vec![StackItem::from("not uint160")],
        });
        assert!(iter.next(10).is_err());

        let h1 = Uint160::new([1, 2, 3]);
        let h2 = Uint160::new([3, 2, 1]);
        ta.lock().unwrap().res = Some(Invoke {
            stack: vec![StackItem::from(h1.to_bytes_be()), StackItem::from(h2.to_bytes_be())],
        });
        let vals = iter.next(10).unwrap();
        assert_eq!(vals, vec![h1, h2]);

        ta.lock().unwrap().err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
        assert!(iter.next(1).is_err());

        assert!(iter.terminate().is_err());

        // Value-based iterator.
        ta.lock().unwrap().err = None;
        ta.lock().unwrap().res = Some(Invoke {
            state: "HALT".to_string(),
            stack: vec![StackItem::new_interop(result::Iterator {
                values: vec![StackItem::from(h1.to_bytes_be()), StackItem::from(h2.to_bytes_be())],
            })],
        });
        iter = fun(&[1]).unwrap();

        ta.lock().unwrap().err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
        assert!(iter.terminate().is_ok());
    }
}

#[test]
fn test_divisible_transfer() {
    let ta = Arc::new(Mutex::new(TestAct::new()));
    let tok = NewDivisible(ta.clone(), Uint160::new([1, 2, 3]));

    ta.lock().unwrap().err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
    assert!(tok.transfer_d(Uint160::new([1, 2, 3]), Uint160::new([3, 2, 1]), &BigInt::from(10), &[3, 2, 1], None).is_err());

    ta.lock().unwrap().err = None;
    ta.lock().unwrap().txh = Some(Uint256::new([1, 2, 3]));
    ta.lock().unwrap().vub = Some(42);
    let (h, vub) = tok.transfer_d(Uint160::new([1, 2, 3]), Uint160::new([3, 2, 1]), &BigInt::from(10), &[3, 2, 1], None).unwrap();
    assert_eq!(h, ta.lock().unwrap().txh.unwrap());
    assert_eq!(vub, ta.lock().unwrap().vub.unwrap());

    ta.lock().unwrap().err = None;
    ta.lock().unwrap().txh = Some(Uint256::new([1, 2, 3]));
    ta.lock().unwrap().vub = Some(42);
    let (h, vub) = tok.transfer_d(Uint160::new([1, 2, 3]), Uint160::new([3, 2, 1]), &BigInt::from(10), &[3, 2, 1], Some(&tData {
        some_int: 5,
        some_string: "ur".to_string(),
    })).unwrap();
    assert_eq!(h, ta.lock().unwrap().txh.unwrap());
    assert_eq!(vub, ta.lock().unwrap().vub.unwrap());

    assert!(tok.transfer_d(Uint160::new([1, 2, 3]), Uint160::new([3, 2, 1]), &BigInt::from(10), &[3, 2, 1], Some(StackItem::new_interop(None))).is_err());
}

#[test]
fn test_divisible_transfer_transaction() {
    let ta = Arc::new(Mutex::new(TestAct::new()));
    let tok = NewDivisible(ta.clone(), Uint160::new([1, 2, 3]));

    let tests: Vec<Box<dyn Fn(Uint160, Uint160, &BigInt, &[u8], Option<&dyn Any>) -> Result<Transaction, Box<dyn Error>>>> = vec![
        Box::new(move |a, b, c, d, e| tok.transfer_d_transaction(a, b, c, d, e)),
        Box::new(move |a, b, c, d, e| tok.transfer_d_unsigned(a, b, c, d, e)),
    ];

    for fun in tests {
        ta.lock().unwrap().err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
        assert!(fun(Uint160::new([1, 2, 3]), Uint160::new([3, 2, 1]), &BigInt::from(10), &[3, 2, 1], None).is_err());

        ta.lock().unwrap().err = None;
        ta.lock().unwrap().tx = Some(Transaction { nonce: 100500, valid_until_block: 42 });
        let tx = fun(Uint160::new([1, 2, 3]), Uint160::new([3, 2, 1]), &BigInt::from(10), &[3, 2, 1], None).unwrap();
        assert_eq!(tx, ta.lock().unwrap().tx.unwrap());

        ta.lock().unwrap().err = None;
        ta.lock().unwrap().tx = Some(Transaction { nonce: 100500, valid_until_block: 42 });
        let tx = fun(Uint160::new([1, 2, 3]), Uint160::new([3, 2, 1]), &BigInt::from(10), &[3, 2, 1], Some(&tData {
            some_int: 5,
            some_string: "ur".to_string(),
        })).unwrap();
        assert_eq!(tx, ta.lock().unwrap().tx.unwrap());

        assert!(fun(Uint160::new([1, 2, 3]), Uint160::new([3, 2, 1]), &BigInt::from(10), &[3, 2, 1], Some(StackItem::new_interop(None))).is_err());
    }
}
