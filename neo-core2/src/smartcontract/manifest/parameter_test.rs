use std::collections::HashMap;
use neo_core2::smartcontract::{self, ParamType};
use neo_core2::vm::stackitem::{Item, StructItem};
use neo_types::BigInt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameters_are_valid() {
        let mut ps = Parameters::new();
        assert!(ps.are_valid().is_ok()); // No parameters.

        ps.push(Parameter::default());
        assert!(ps.are_valid().is_err());

        ps[0].name = "qwerty".to_string();
        assert!(ps.are_valid().is_ok());

        ps[0].param_type = ParamType::try_from(0x42u8).unwrap_err(); // Invalid type.
        assert!(ps.are_valid().is_err());

        ps[0].param_type = ParamType::Void;
        assert!(ps.are_valid().is_err());

        ps[0].param_type = ParamType::Bool;
        assert!(ps.are_valid().is_ok());

        ps.push(Parameter { name: "qwerty".to_string(), param_type: ParamType::Bool });
        assert!(ps.are_valid().is_err());
    }

    #[test]
    fn test_parameter_to_from_stack_item() {
        let p = Parameter {
            name: "param".to_string(),
            param_type: ParamType::String,
        };
        let expected = Item::Struct(StructItem::new(vec![
            Item::ByteArray(p.name.as_bytes().to_vec()),
            Item::Integer(BigInt::from(p.param_type as i32)),
        ]));
        check_to_from_stack_item(&p, &expected);
    }

    #[test]
    fn test_parameter_from_stack_item_errors() {
        let err_cases: HashMap<&str, Item> = [
            ("not a struct", Item::Array(vec![])),
            ("invalid length", Item::Struct(StructItem::new(vec![]))),
            ("invalid name type", Item::Struct(StructItem::new(vec![Item::Interop(Box::new(())), Item::Null]))),
            ("invalid type type", Item::Struct(StructItem::new(vec![Item::ByteArray(vec![]), Item::Null]))),
            ("invalid type value", Item::Struct(StructItem::new(vec![Item::ByteArray(vec![]), Item::Integer(BigInt::from(-100500))]))),
        ].iter().cloned().collect();

        for (name, err_case) in err_cases {
            let mut p = Parameter::default();
            assert!(p.from_stack_item(&err_case).is_err(), "{}", name);
        }
    }
}

fn check_to_from_stack_item(p: &Parameter, expected: &Item) {
    let item = p.to_stack_item();
    assert_eq!(&item, expected);

    let mut p2 = Parameter::default();
    assert!(p2.from_stack_item(&item).is_ok());
    assert_eq!(p, &p2);
}
