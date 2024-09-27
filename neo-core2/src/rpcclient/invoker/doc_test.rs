use std::sync::Arc;
use std::sync::Mutex;
use std::error::Error;
use std::collections::HashMap;

use crate::core::transaction::{self, Transaction};
use crate::encoding::address;
use crate::rpcclient::{self, invoker, neo, unwrap};
use crate::util::Uint160;
use crate::vm::vmstate::VMState;

fn example_invoker() -> Result<(), Box<dyn Error>> {
    // No error checking done at all, intentionally.
    let c = rpcclient::Client::new("url", rpcclient::Options::default())?;

    // A simple invoker with no signers, perfectly fine for reads from safe methods.
    let inv = invoker::Invoker::new(Arc::new(Mutex::new(c.clone())), None);

    // Get the NEO token supply (notice that unwrap is used to get the result).
    let supply = unwrap::big_int(inv.call(neo::HASH, "totalSupply")?)?;
    println!("Supply: {}", supply);

    let acc = address::string_to_uint160("NVTiAjNgagDkTr5HTzDmQP9kPwPHN5BgVq")?;
    // Get the NEO balance for account NVTiAjNgagDkTr5HTzDmQP9kPwPHN5BgVq.
    let balance = unwrap::big_int(inv.call(neo::HASH, "balanceOf", &[acc.clone()])?)?;
    println!("Balance: {}", balance);

    // Test-invoke transfer call.
    let res = inv.call(neo::HASH, "transfer", &[acc.clone(), Uint160::new([1, 2, 3]), 1.into(), None])?;
    if res.state == VMState::Halt.to_string() {
        panic!("NEO is broken!"); // inv has no signers and transfer requires a witness to be performed.
    } else {
        println!("ok"); // this actually should fail
    }

    // A historic invoker with no signers at block 1000000.
    let inv = invoker::Invoker::new_historic_at_height(1000000, Arc::new(Mutex::new(c.clone())), None);

    // It's the same call as above, but the data is for a state at block 1000000.
    let balance = unwrap::big_int(inv.call(neo::HASH, "balanceOf", &[acc.clone()])?)?;
    println!("Historic Balance: {}", balance);

    // This invoker has a signer for NVTiAjNgagDkTr5HTzDmQP9kPwPHN5BgVq account with
    // CalledByEntry scope, which is sufficient for most operation. It uses current
    // state which is exactly what you need if you want to then create a transaction
    // with the same action.
    let inv = invoker::Invoker::new(Arc::new(Mutex::new(c.clone())), Some(vec![transaction::Signer {
        account: acc.clone(),
        scopes: transaction::CalledByEntry,
    }]));

    // Now test invocation should be fine (if NVTiAjNgagDkTr5HTzDmQP9kPwPHN5BgVq has 1 NEO of course).
    let res = inv.call(neo::HASH, "transfer", &[acc.clone(), Uint160::new([1, 2, 3]), 1.into(), None])?;
    if res.state == VMState::Halt.to_string() {
        // transfer actually returns a value, so check it too.
        let ok = unwrap::bool(res, None)?;
        if ok {
            // OK, as expected.
            // res.script contains the corresponding script.
            println!("Script: {:?}", res.script);
            // res.gas_consumed has an appropriate system fee required for a transaction.
            println!("Gas Consumed: {}", res.gas_consumed);
        }
    }

    // Now let's try working with iterators.
    let nep11_contract = Uint160::new([1, 2, 3]);

    let mut tokens: Vec<Vec<u8>> = Vec::new();

    // Try doing it the right way, by traversing the iterator.
    let (sess, iter) = unwrap::session_iterator(inv.call(nep11_contract, "tokensOf", &[acc.clone()])?)?;

    // The server doesn't support sessions and doesn't perform iterator expansion,
    // iterators can't be used.
    if let Err(err) = iter {
        if err == unwrap::Error::NoSessionID {
            // But if we expect some low number of elements, CallAndExpandIterator
            // can help us in this case. If the account has more than 10 elements,
            // some of them will be missing from the response.
            tokens = unwrap::array_of_bytes(inv.call_and_expand_iterator(nep11_contract, "tokensOf", 10, &[acc.clone()])?)?;
        } else {
            panic!("some error");
        }
    } else {
        let mut items = inv.traverse_iterator(sess.clone(), &iter, 100)?;
        // Keep going until there are no more elements
        while !items.is_empty() {
            for itm in items {
                let token_id = itm.try_bytes()?;
                tokens.push(token_id);
            }
            items = inv.traverse_iterator(sess.clone(), &iter, 100)?;
        }
        // Let the server release the session.
        inv.terminate_session(sess)?;
    }
    println!("Tokens: {:?}", tokens);
    Ok(())
}
