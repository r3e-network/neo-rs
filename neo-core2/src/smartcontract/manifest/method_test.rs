use std::str::FromStr;
use neo_core2::crypto::keys::{PrivateKey, PublicKey};
use neo_core2::smartcontract::{ParamType, StackItem};
use neo_core2::smartcontract::manifest::{Method, Parameter, Group};

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::TryFrom;

    #[test]
    fn test_method_is_valid() {
        let mut m = Method::default();
        assert!(m.is_valid().is_err()); // No name.

        m.name = "qwerty".to_string();
        assert!(m.is_valid().is_ok());

        m.offset = -100;
        assert!(m.is_valid().is_err());

        m.offset = 100;
        m.return_type = ParamType::try_from(0x42u8).unwrap_err(); // Invalid type.
        assert!(m.is_valid().is_err());

        m.return_type = ParamType::Bool;
        assert!(m.is_valid().is_ok());

        m.parameters = vec![
            Parameter::new("param".to_string(), ParamType::Bool),
            Parameter::new("param".to_string(), ParamType::Bool),
        ];
        assert!(m.is_valid().is_err());
    }

    #[test]
    fn test_method_to_from_stack_item() {
        let m = Method {
            name: "mur".to_string(),
            offset: 5,
            parameters: vec![Parameter::new("p1".to_string(), ParamType::Bool)],
            return_type: ParamType::String,
            safe: true,
        };
        let expected = StackItem::Struct(vec![
            StackItem::ByteArray(m.name.as_bytes().to_vec()),
            StackItem::Array(vec![
                StackItem::Struct(vec![
                    StackItem::ByteArray(m.parameters[0].name.as_bytes().to_vec()),
                    StackItem::Integer(m.parameters[0].param_type as i64),
                ]),
            ]),
            StackItem::Integer(m.return_type as i64),
            StackItem::Integer(m.offset as i64),
            StackItem::Boolean(m.safe),
        ]);
        check_to_from_stack_item(&m, &expected);
    }

    #[test]
    fn test_method_from_stack_item_errors() {
        let err_cases = vec![
            ("not a struct", StackItem::Array(vec![])),
            ("invalid length", StackItem::Struct(vec![])),
            ("invalid name type", StackItem::Struct(vec![
                StackItem::Interop(Box::new(())),
                StackItem::Null,
                StackItem::Null,
                StackItem::Null,
                StackItem::Null,
            ])),
            ("invalid parameters type", StackItem::Struct(vec![
                StackItem::ByteArray(vec![]),
                StackItem::Null,
                StackItem::Null,
                StackItem::Null,
                StackItem::Null,
            ])),
            ("invalid parameter", StackItem::Struct(vec![
                StackItem::ByteArray(vec![]),
                StackItem::Array(vec![StackItem::Struct(vec![])]),
                StackItem::Null,
                StackItem::Null,
                StackItem::Null,
            ])),
            ("invalid return type", StackItem::Struct(vec![
                StackItem::ByteArray(vec![]),
                StackItem::Array(vec![]),
                StackItem::Null,
                StackItem::Null,
                StackItem::Null,
            ])),
            ("invalid offset", StackItem::Struct(vec![
                StackItem::ByteArray(vec![]),
                StackItem::Array(vec![]),
                StackItem::Integer(1),
                StackItem::Interop(Box::new(())),
                StackItem::Null,
            ])),
            ("invalid safe", StackItem::Struct(vec![
                StackItem::ByteArray(vec![]),
                StackItem::Array(vec![]),
                StackItem::Integer(1),
                StackItem::Integer(5),
                StackItem::Interop(Box::new(())),
            ])),
        ];

        for (name, err_case) in err_cases {
            let result = Method::try_from(&err_case);
            assert!(result.is_err(), "{} should be an error", name);
        }
    }

    #[test]
    fn test_group_to_from_stack_item() {
        let pk = PrivateKey::new().public_key();
        let g = Group {
            public_key: pk,
            signature: vec![0; 64], // Assuming SignatureLen is 64
        };
        let expected = StackItem::Struct(vec![
            StackItem::ByteArray(pk.to_bytes()),
            StackItem::ByteArray(vec![0; 64]),
        ]);
        check_to_from_stack_item(&g, &expected);
    }

    #[test]
    fn test_group_from_stack_item_errors() {
        let pk = PrivateKey::new().public_key();
        let err_cases = vec![
            ("not a struct", StackItem::Array(vec![])),
            ("invalid length", StackItem::Struct(vec![])),
            ("invalid pub type", StackItem::Struct(vec![
                StackItem::Interop(Box::new(())),
                StackItem::Null,
            ])),
            ("invalid pub bytes", StackItem::Struct(vec![
                StackItem::ByteArray(vec![1]),
                StackItem::Null,
            ])),
            ("invalid sig type", StackItem::Struct(vec![
                StackItem::ByteArray(pk.to_bytes()),
                StackItem::Interop(Box::new(())),
            ])),
            ("invalid sig len", StackItem::Struct(vec![
                StackItem::ByteArray(pk.to_bytes()),
                StackItem::ByteArray(vec![1]),
            ])),
        ];

        for (name, err_case) in err_cases {
            let result = Group::try_from(&err_case);
            assert!(result.is_err(), "{} should be an error", name);
        }
    }

    fn check_to_from_stack_item<T: TryFrom<&StackItem> + Into<StackItem>>(item: &T, expected: &StackItem) {
        let stack_item: StackItem = item.clone().into();
        assert_eq!(&stack_item, expected);

        let roundtrip = T::try_from(&stack_item).expect("Failed to convert back from StackItem");
        assert_eq!(&Into::<StackItem>::into(roundtrip), expected);
    }
}
