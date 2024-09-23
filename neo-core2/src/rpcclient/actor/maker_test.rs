use std::collections::HashMap;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::error::Error;

use crate::rpcclient::actor::{NewSimple, NewTuned, Options, test_rpc_and_account};
use crate::transaction::{Transaction, Attribute, Signer};
use crate::util::Uint160;
use crate::result::Invoke;
use crate::require;

#[test]
fn test_calculate_valid_until_block() -> Result<(), Box<dyn Error>> {
    let (client, acc) = test_rpc_and_account()?;
    let mut a = NewSimple(client.clone(), acc.clone())?;
    
    client.err = Some("error".to_string());
    let err = a.calculate_valid_until_block();
    assert!(err.is_err());

    client.err = None;
    client.b_count.store(42, Ordering::SeqCst);
    let vub = a.calculate_valid_until_block()?;
    assert_eq!(42 + 7 + 1, vub);

    client.version.protocol.validators_history = HashMap::from([
        (0, 7),
        (40, 4),
        (80, 10),
    ]);
    a = NewSimple(client.clone(), acc.clone())?;
    
    let vub = a.calculate_valid_until_block()?;
    assert_eq!(42 + 4 + 1, vub);

    client.b_count.store(101, Ordering::SeqCst);
    let vub = a.calculate_valid_until_block()?;
    assert_eq!(101 + 10 + 1, vub);

    Ok(())
}

#[test]
fn test_make_unsigned() -> Result<(), Box<dyn Error>> {
    let (client, acc) = test_rpc_and_account()?;
    let mut a = NewSimple(client.clone(), acc.clone())?;

    // Bad parameters.
    let script = vec![1, 2, 3];
    let err = a.make_unsigned_unchecked_run(&script, -1, None);
    assert!(err.is_err());
    let err = a.make_unsigned_unchecked_run(&[], 1, None);
    assert!(err.is_err());
    let err = a.make_unsigned_unchecked_run(None, 1, None);
    assert!(err.is_err());

    // RPC error.
    client.err = Some("err".to_string());
    let err = a.make_unsigned_unchecked_run(&script, 1, None);
    assert!(err.is_err());

    // Good unchecked.
    client.net_fee = 42;
    client.b_count.store(100500, Ordering::SeqCst);
    client.err = None;
    let tx = a.make_unsigned_unchecked_run(&script, 1, None)?;
    assert_eq!(script, tx.script);
    assert_eq!(1, tx.signers.len());
    assert_eq!(acc.contract.script_hash(), tx.signers[0].account);
    assert_eq!(1, tx.scripts.len());
    assert_eq!(acc.contract.script, tx.scripts[0].verification_script);
    assert!(tx.scripts[0].invocation_script.is_none());

    // Bad run.
    client.err = Some("".to_string());
    let err = a.make_unsigned_run(&script, None);
    assert!(err.is_err());

    // Faulted run.
    client.inv_res = Some(Invoke { state: "FAULT".to_string(), gas_consumed: 3, script: script.clone() });
    client.err = None;
    let err = a.make_unsigned_run(&script, None);
    assert!(err.is_err());

    // Good run.
    client.inv_res = Some(Invoke { state: "HALT".to_string(), gas_consumed: 3, script: script.clone() });
    let tx = a.make_unsigned_run(&script, None)?;
    assert!(tx.is_ok());

    // Tuned.
    let opts = Options {
        attributes: vec![Attribute { attr_type: transaction::HIGH_PRIORITY }],
    };
    a = NewTuned(client.clone(), a.signers.clone(), opts)?;
    let tx = a.make_unsigned_run(&script, None)?;
    assert!(tx.has_attribute(transaction::HIGH_PRIORITY));

    Ok(())
}

#[test]
fn test_make_signed() -> Result<(), Box<dyn Error>> {
    let (client, acc) = test_rpc_and_account()?;
    let mut a = NewSimple(client.clone(), acc.clone())?;

    // Bad script.
    let err = a.make_unchecked_run(None, 0, None, None);
    assert!(err.is_err());

    // Good, no hook.
    let script = vec![1, 2, 3];
    let tx = a.make_unchecked_run(&script, 0, None, None)?;
    assert!(tx.is_ok());

    // Bad, can't sign because of a hook.
    let err = a.make_unchecked_run(&script, 0, None, Some(|t: &mut Transaction| {
        t.signers.push(Signer::default());
        Ok(())
    }));
    assert!(err.is_err());

    // Bad, hook returns an error.
    let err = a.make_unchecked_run(&script, 0, None, Some(|_t: &mut Transaction| {
        Err("".into())
    }));
    assert!(err.is_err());

    // Good with a hook.
    let tx = a.make_unchecked_run(&script, 0, None, Some(|t: &mut Transaction| {
        t.valid_until_block = 777;
        Ok(())
    }))?;
    assert_eq!(777, tx.valid_until_block);

    // Tuned.
    let opts = Options {
        modifier: Some(|t: &mut Transaction| {
            t.valid_until_block = 888;
            Ok(())
        }),
    };
    let at = NewTuned(client.clone(), a.signers.clone(), opts)?;
    let tx = at.make_unchecked_run(&script, 0, None, None)?;
    assert_eq!(888, tx.valid_until_block);

    // Checked

    // Bad, invocation fails.
    client.err = Some("".to_string());
    let err = a.make_tuned_run(&script, None, Some(|_r: &Invoke, _t: &mut Transaction| {
        Ok(())
    }));
    assert!(err.is_err());

    // Bad, hook returns an error.
    client.err = None;
    client.inv_res = Some(Invoke { state: "HALT".to_string(), gas_consumed: 3, script: script.clone() });
    let err = a.make_tuned_run(&script, None, Some(|_r: &Invoke, _t: &mut Transaction| {
        Err("".into())
    }));
    assert!(err.is_err());

    // Good, no hook.
    let tx = a.make_tuned_run(&script, Some(vec![Attribute { attr_type: transaction::HIGH_PRIORITY }]), None)?;
    assert!(tx.is_ok());
    let tx = a.make_run(&script)?;
    assert!(tx.is_ok());

    // Bad, invocation returns FAULT.
    client.inv_res = Some(Invoke { state: "FAULT".to_string(), gas_consumed: 3, script: script.clone() });
    let err = a.make_tuned_run(&script, None, None);
    assert!(err.is_err());

    // Good, invocation returns FAULT, but callback ignores it.
    let tx = a.make_tuned_run(&script, None, Some(|_r: &Invoke, _t: &mut Transaction| {
        Ok(())
    }))?;
    assert!(tx.is_ok());

    // Good, via call and with a callback.
    let tx = a.make_tuned_call(Uint160::default(), "something", Some(vec![Attribute { attr_type: transaction::HIGH_PRIORITY }]), Some(|_r: &Invoke, _t: &mut Transaction| {
        Ok(())
    }), vec!["param".into(), 1.into()])?;
    assert!(tx.is_ok());

    // Bad, it still is a FAULT.
    let err = a.make_call(Uint160::default(), "method");
    assert!(err.is_err());

    // Good.
    client.inv_res = Some(Invoke { state: "HALT".to_string(), gas_consumed: 3, script: script.clone() });
    let tx = a.make_call(Uint160::default(), "method", vec![1.into()])?;
    assert!(tx.is_ok());

    // Tuned.
    let opts = Options {
        checker_modifier: Some(|_r: &Invoke, t: &mut Transaction| {
            t.valid_until_block = 888;
            Ok(())
        }),
    };
    let at = NewTuned(client.clone(), a.signers.clone(), opts)?;
    let tx = at.make_run(&script)?;
    assert_eq!(888, tx.valid_until_block);

    Ok(())
}
