use std::any::Any;
use std::collections::HashMap;
use std::panic::Location;

use crate::core::interop;
use crate::core::interop::storage;
use crate::core::interop::Context;
use crate::test_helpers::create_vm;
use rstest::rstest;
use rstest_reuse::{self, *};

#[rstest]
#[case("int", Box::new(1) as Box<dyn Any>)]
#[case("bool", Box::new(false) as Box<dyn Any>)]
#[case("string", Box::new("smth") as Box<dyn Any>)]
#[case("array", Box::new(vec![1, 2, 3]) as Box<dyn Any>)]
fn test_unexpected_non_interops(#[case] key: &str, #[case] value: Box<dyn Any>) {
    let funcs: Vec<fn(&mut Context) -> Result<(), String>> = vec![
        storage::context_as_read_only,
        storage::delete,
        storage::find,
        storage::get,
        storage::put,
    ];

    for f in funcs {
        let fname = Location::caller().to_string();
        let test_name = format!("{}/{}", key, fname);
        let (mut vm, mut ic, _) = create_vm();
        vm.estack().push_val(value);
        assert!(f(&mut ic).is_err(), "{}", test_name);
    }
}
