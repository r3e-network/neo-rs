use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use std::sync::Mutex;

use crate::neorpc::result::Invoke;
use crate::util::Uint160;
use crate::vm::stackitem;
use crate::nep11::{NewNonDivisibleReader, NewNonDivisible};
use crate::test_utils::testAct;
use anyhow::Result;

#[test]
fn test_nd_owner_of() -> Result<()> {
    let ta = Arc::new(Mutex::new(testAct::new()));
    let tr = NewNonDivisibleReader(ta.clone(), Uint160::from([1, 2, 3]));
    let tt = NewNonDivisible(ta.clone(), Uint160::from([1, 2, 3]));

    let mut map: HashMap<&str, fn(&[u8]) -> Result<Uint160, Box<dyn Error>>> = HashMap::new();
    map.insert("Reader", tr.owner_of);
    map.insert("Full", tt.owner_of);

    for (name, fun) in map {
        let ta = ta.clone();
        let fun = fun.clone();
        std::thread::spawn(move || {
            let mut ta = ta.lock().unwrap();
            ta.err = Some(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "")));
            let res = fun(&[3, 2, 1]);
            assert!(res.is_err());

            ta.err = None;
            ta.res = Some(Invoke {
                state: "HALT".to_string(),
                stack: vec![stackitem::Item::from(100500)],
            });
            let res = fun(&[3, 2, 1]);
            assert!(res.is_err());

            let own = Uint160::from([1, 2, 3]);
            ta.res = Some(Invoke {
                state: "HALT".to_string(),
                stack: vec![stackitem::Item::from(own.to_bytes_be())],
            });
            let res = fun(&[3, 2, 1]);
            assert!(res.is_ok());
            assert_eq!(own, res.unwrap());
        }).join().unwrap();
    }
    Ok(())
}
