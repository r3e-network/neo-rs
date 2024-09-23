use std::collections::HashMap;
use std::sync::Arc;

use neo_core2::smartcontract::{ParamType, Parameter};
use neo_core2::vm::stackitem::{Item, StackItem};
use neo_core2::smartcontract::manifest::Event;

#[cfg(test)]
mod tests {
    use super::*;
    use neo_core2::vm::stackitem::{Array, ByteArray, Integer, Struct};

    #[test]
    fn test_event_is_valid() {
        let mut e = Event::default();
        assert!(e.is_valid().is_err());

        e.name = "some".to_string();
        assert!(e.is_valid().is_ok());

        e.parameters = Vec::new();
        assert!(e.is_valid().is_ok());

        e.parameters.push(Parameter::new("p1".to_string(), ParamType::Boolean));
        assert!(e.is_valid().is_ok());

        e.parameters.push(Parameter::new("p2".to_string(), ParamType::Integer));
        assert!(e.is_valid().is_ok());

        e.parameters.push(Parameter::new("p3".to_string(), ParamType::Integer));
        assert!(e.is_valid().is_ok());

        e.parameters.push(Parameter::new("p1".to_string(), ParamType::Integer));
        assert!(e.is_valid().is_err());
    }

    #[test]
    fn test_event_to_from_stack_item() {
        let m = Event {
            name: "mur".to_string(),
            parameters: vec![Parameter::new("p1".to_string(), ParamType::Boolean)],
        };
        let expected = Arc::new(Struct::new(vec![
            Arc::new(ByteArray::new(m.name.as_bytes().to_vec())),
            Arc::new(Array::new(vec![Arc::new(Struct::new(vec![
                Arc::new(ByteArray::new(m.parameters[0].name.as_bytes().to_vec())),
                Arc::new(Integer::new(m.parameters[0].param_type as i64)),
            ]))])),
        ]));
        check_to_from_stack_item(&m, expected);
    }

    #[test]
    fn test_event_from_stack_item_errors() {
        let err_cases: HashMap<&str, Arc<dyn Item>> = [
            ("not a struct", Arc::new(Array::new(vec![]))),
            ("invalid length", Arc::new(Struct::new(vec![]))),
            ("invalid name type", Arc::new(Struct::new(vec![
                Arc::new(StackItem::Interop(())),
                Arc::new(StackItem::Null),
            ]))),
            ("invalid parameters type", Arc::new(Struct::new(vec![
                Arc::new(ByteArray::new(vec![])),
                Arc::new(StackItem::Null),
            ]))),
            ("invalid parameter", Arc::new(Struct::new(vec![
                Arc::new(ByteArray::new(vec![])),
                Arc::new(Array::new(vec![Arc::new(Struct::new(vec![]))])),
            ]))),
        ].iter().cloned().collect();

        for (name, err_case) in err_cases {
            let mut p = Event::default();
            assert!(p.from_stack_item(err_case).is_err(), "{}", name);
        }
    }

    #[test]
    fn test_event_check_compliance() {
        let m = Event {
            name: "mur".to_string(),
            parameters: vec![Parameter::new("p1".to_string(), ParamType::Boolean)],
        };
        assert!(m.check_compliance(&[]).is_err());
        assert!(m.check_compliance(&[Arc::new(StackItem::from("something"))]).is_err());
        assert!(m.check_compliance(&[Arc::new(StackItem::from(true))]).is_ok());
    }

    fn check_to_from_stack_item(m: &Event, expected: Arc<dyn Item>) {
        let item = m.to_stack_item();
        assert_eq!(item, expected);

        let mut deserialized = Event::default();
        assert!(deserialized.from_stack_item(item).is_ok());
        assert_eq!(deserialized, *m);
    }
}
