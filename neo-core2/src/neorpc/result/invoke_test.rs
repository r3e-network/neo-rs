use base64;
use serde_json;
use std::error::Error;
use std::sync::Arc;
use uuid::Uuid;

use neo_core2::core::state::{NotificationEvent, Execution};
use neo_core2::core::transaction::{Transaction, Signer, Witness};
use neo_core2::smartcontract::trigger;
use neo_core2::util::{Uint160, Uint256};
use neo_core2::vm::stackitem::{Item, BigInteger};
use neo_core2::vm::vmstate::VMState;
use neo_core2::neorpc::result::{Invoke, AppExecToInvocation};

#[test]
fn test_invoke_marshal_json() -> Result<(), Box<dyn Error>> {
    let mut tx = Transaction::new(vec![1, 2, 3, 4], 0);
    tx.signers = vec![Signer { account: Uint160::from_slice(&[1, 2, 3]) }];
    tx.scripts = vec![Witness { invocation_script: vec![], verification_script: vec![] }];
    tx.size();
    tx.hash();

    let result = Invoke {
        state: "HALT".to_string(),
        gas_consumed: 237626000,
        script: vec![10],
        stack: vec![Item::BigInteger(BigInteger::new(1.into()))],
        fault_exception: "".to_string(),
        notifications: vec![],
        transaction: Some(tx.clone()),
    };

    let data = serde_json::to_string(&result)?;
    let expected = format!(r#"{{
        "state":"HALT",
        "gasconsumed":"237626000",
        "script":"{}",
        "stack":[
            {{"type":"Integer","value":"1"}}
        ],
        "notifications":[],
        "exception": null,
        "tx":"{}"
    }}"#, base64::encode(&result.script), base64::encode(&tx.to_bytes()));

    assert_eq!(expected, data);

    let actual: Invoke = serde_json::from_str(&data)?;
    assert_eq!(result, actual);

    Ok(())
}

#[test]
fn test_app_exec_to_invocation() -> Result<(), Box<dyn Error>> {
    // With error.
    let some_err = "some err".to_string();
    let result = AppExecToInvocation(None, Some(some_err.clone()));
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), some_err);

    // Good.
    let h = Uint256::from_slice(&[1, 2, 3]);
    let ex = Execution {
        trigger: trigger::Application,
        vm_state: VMState::Fault,
        gas_consumed: 123,
        stack: vec![Item::BigInteger(BigInteger::new(123.into()))],
        events: vec![NotificationEvent {
            script_hash: Uint160::from_slice(&[3, 2, 1]),
            name: "Notification".to_string(),
            item: Item::Array(vec![Item::Null]),
        }],
        fault_exception: "some fault exception".to_string(),
    };
    let inv = AppExecToInvocation(Some(Arc::new(ex.clone())), None)?;
    assert_eq!(ex.vm_state.to_string(), inv.state);
    assert_eq!(ex.gas_consumed, inv.gas_consumed);
    assert!(inv.script.is_none());
    assert_eq!(ex.stack, inv.stack);
    assert_eq!(ex.fault_exception, inv.fault_exception);
    assert_eq!(ex.events, inv.notifications);
    assert!(inv.transaction.is_none());
    assert!(inv.diagnostics.is_none());
    assert_eq!(Uuid::nil(), inv.session);

    Ok(())
}
