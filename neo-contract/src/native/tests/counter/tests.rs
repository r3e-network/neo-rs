use neo_base::Bytes;

use crate::{
    native::{tests::counter::contract::CounterContract, NativeRegistry},
    runtime::{execution::ExecutionContext, Value},
};

use super::fixtures::{sample_context, sample_manifest};

#[test]
fn invoke_native_contract() {
    let manifest = sample_manifest();
    let registry = NativeRegistry::new();
    registry.register(CounterContract { manifest });

    let (mut store, mut ctx) = sample_context();
    let key = Bytes::from(vec![1, 2, 3, 4]);
    let result = registry
        .invoke(
            "Counter",
            "increment",
            &mut ctx,
            &[Value::Bytes(key.clone())],
        )
        .expect("invoke succeeds");

    assert_eq!(&result.value, &Value::Int(1));

    let reset = registry
        .invoke("Counter", "reset", &mut ctx, &[Value::Bytes(key)])
        .expect("reset succeeds");
    assert_eq!(&reset.value, &Value::Bytes(Bytes::default()));
}
