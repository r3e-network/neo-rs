use std::collections::HashMap;
use hex;
use rand::Rng;
use neo_core::interop::{self, Context};
use neo_core::state::NotificationEvent;
use neo_core::util::Uint160;
use neo_core::vm::{self, stackitem::{self, StackItem}};
use neo_core::smartcontract::callflag;
use neo_core::internal::random;
use neo_core::murmur128;
use neo_core::require;

#[test]
fn test_gas_left() {
    let mut ic = Context { vm: vm::VM::new(), ..Default::default() };
    ic.vm.gas_limit = -1;
    ic.vm.add_gas(58);
    require::no_error(gas_left(&mut ic));
    check_stack(&ic.vm, -1);

    let mut ic = Context { vm: vm::VM::new(), ..Default::default() };
    ic.vm.gas_limit = 100;
    ic.vm.add_gas(58);
    require::no_error(gas_left(&mut ic));
    check_stack(&ic.vm, 42);
}

#[test]
fn test_runtime_get_notifications() {
    let v = vm::VM::new();
    let ic = Context {
        vm: v,
        notifications: vec![
            NotificationEvent { script_hash: Uint160::new([1; 20]), name: "Event1".to_string(), item: stackitem::Array::new(vec![stackitem::ByteArray::new(vec![11])]) },
            NotificationEvent { script_hash: Uint160::new([2; 20]), name: "Event2".to_string(), item: stackitem::Array::new(vec![stackitem::ByteArray::new(vec![22])]) },
            NotificationEvent { script_hash: Uint160::new([1; 20]), name: "Event1".to_string(), item: stackitem::Array::new(vec![stackitem::ByteArray::new(vec![33])]) },
        ],
        ..Default::default()
    };

    v.estack().push_val(stackitem::Null::new());
    require::no_error(get_notifications(&ic));

    let arr = v.estack().pop().array();
    require::equal(ic.notifications.len(), arr.len());
    for (i, elem) in arr.iter().enumerate() {
        let elem = elem.value().as_array().unwrap();
        require::equal(ic.notifications[i].script_hash.bytes_be(), elem[0].value());
        let name = stackitem::to_string(&elem[1]).unwrap();
        require::equal(ic.notifications[i].name, name);
        ic.notifications[i].item.mark_as_read_only();
        require::equal(ic.notifications[i].item, elem[2]);
    }

    let h = Uint160::new([2; 20]).bytes_be();
    v.estack().push_val(h.clone());
    require::no_error(get_notifications(&ic));

    let arr = v.estack().pop().array();
    require::equal(1, arr.len());
    let elem = arr[0].value().as_array().unwrap();
    require::equal(h, elem[0].value());
    let name = stackitem::to_string(&elem[1]).unwrap();
    require::equal(ic.notifications[1].name, name);
    require::equal(ic.notifications[1].item, elem[2]);

    v.estack().push_val(stackitem::Interop::new(Uint160::new([1; 20])));
    require::error(get_notifications(&ic));

    v.estack().push_val(vec![1, 2, 3]);
    require::error(get_notifications(&ic));

    for _ in 0..vm::MAX_STACK_SIZE + 1 {
        ic.notifications.push(NotificationEvent {
            script_hash: Uint160::new([3; 20]),
            name: "Event3".to_string(),
            item: stackitem::Array::new(vec![]),
        });
    }
    v.estack().push_val(stackitem::Null::new());
    require::error(get_notifications(&ic));
}

#[test]
fn test_runtime_get_invocation_counter() {
    let mut ic = Context { vm: vm::VM::new(), invocations: HashMap::new(), ..Default::default() };
    let h = random::uint160();
    ic.invocations.insert(h.clone(), 42);

    let mut h1 = h.clone();
    h1[0] ^= 0xFF;
    ic.vm.load_script_with_hash(vec![1], h1, callflag::NONE_FLAG);
    require::no_error(get_invocation_counter(&ic));
    check_stack(&ic.vm, 1);

    ic.vm.load_script_with_hash(vec![1], h, callflag::NONE_FLAG);
    require::no_error(get_invocation_counter(&ic));
    check_stack(&ic.vm, 42);
}

#[test]
fn test_murmur_compat() {
    let res = murmur128(b"hello", 123);
    require::equal("0bc59d0ad25fde2982ed65af61227a0e", hex::encode(res));

    let res = murmur128(b"world", 123);
    require::equal("3d3810fed480472bd214a14023bb407f", hex::encode(res));

    let res = murmur128(b"hello world", 123);
    require::equal("e0a0632d4f51302c55e3b3e48d28795d", hex::encode(res));

    let bs = hex::decode("718f952132679baa9c5c2aa0d329fd2a").unwrap();
    let res = murmur128(&bs, 123);
    require::equal("9b4aa747ff0cf4e41b3d96251551c8ae", hex::encode(res));
}
