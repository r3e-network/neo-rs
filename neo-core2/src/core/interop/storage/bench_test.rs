use std::collections::HashMap;
use std::fmt;
use rand::Rng;
use test::Bencher;
use crate::core::interop::iterator;
use crate::core::interop::storage as istorage;
use crate::core::native;
use crate::core::state;
use crate::vm::stackitem;
use crate::test_utils::create_vm_and_contract_state;

#[bench]
fn benchmark_storage_find(b: &mut Bencher) {
    for count in (10..=10000).step_by(10) {
        b.bench_function(&format!("{}Elements", count), |b| {
            let (mut vm, contract_state, mut context, _) = create_vm_and_contract_state();
            native::put_contract_state(&mut context.dao, &contract_state).unwrap();

            let mut items = HashMap::new();
            for _ in 0..count {
                items.insert(format!("abc{}", random_string(10)), random_bytes(10));
            }
            for (k, v) in &items {
                context.dao.put_storage_item(contract_state.id, k.as_bytes(), v);
                context.dao.put_storage_item(contract_state.id + 1, k.as_bytes(), v);
            }
            let changes = context.dao.persist().unwrap();
            assert_ne!(changes, 0);

            b.iter(|| {
                b.stop_timer();
                vm.estack().push_val(istorage::FIND_DEFAULT);
                vm.estack().push_val("abc");
                vm.estack().push_val(stackitem::StackItem::Interop(Box::new(istorage::Context { id: contract_state.id })));
                b.start_timer();
                if let Err(_) = istorage::find(&mut context) {
                    b.fail();
                }
                b.stop_timer();
                context.finalize();
            });
        });
    }
}

#[bench]
fn benchmark_storage_find_iterator_next(b: &mut Bencher) {
    for count in (10..=10000).step_by(10) {
        let cases = vec![
            ("Pick1", 1),
            ("PickHalf", count / 2),
            ("PickAll", count),
        ];
        b.bench_function(&format!("{}Elements", count), |b| {
            for (name, last) in &cases {
                b.bench_function(name, |b| {
                    let (mut vm, contract_state, mut context, _) = create_vm_and_contract_state();
                    native::put_contract_state(&mut context.dao, &contract_state).unwrap();

                    let mut items = HashMap::new();
                    for _ in 0..count {
                        items.insert(format!("abc{}", random_string(10)), random_bytes(10));
                    }
                    for (k, v) in &items {
                        context.dao.put_storage_item(contract_state.id, k.as_bytes(), v);
                        context.dao.put_storage_item(contract_state.id + 1, k.as_bytes(), v);
                    }
                    let changes = context.dao.persist().unwrap();
                    assert_ne!(changes, 0);

                    b.iter(|| {
                        b.stop_timer();
                        vm.estack().push_val(istorage::FIND_DEFAULT);
                        vm.estack().push_val("abc");
                        vm.estack().push_val(stackitem::StackItem::Interop(Box::new(istorage::Context { id: contract_state.id })));
                        b.start_timer();
                        if let Err(_) = istorage::find(&mut context) {
                            b.fail();
                        }
                        b.stop_timer();
                        let res = context.vm.estack().pop().unwrap();
                        for _ in 0..*last {
                            context.vm.estack().push_val(res.clone());
                            b.start_timer();
                            iterator::next(&mut context).unwrap();
                            b.stop_timer();
                            assert!(context.vm.estack().pop().unwrap().as_bool().unwrap());
                        }

                        context.vm.estack().push_val(res);
                        iterator::next(&mut context).unwrap();
                        let actual = context.vm.estack().pop().unwrap().as_bool().unwrap();
                        if *last == count {
                            assert!(!actual);
                        } else {
                            assert!(actual);
                        }
                        context.finalize();
                    });
                });
            }
        });
    }
}

fn random_string(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn random_bytes(len: usize) -> Vec<u8> {
    rand::thread_rng().gen::<[u8; 10]>().to_vec()
}
