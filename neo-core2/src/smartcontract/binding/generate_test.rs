use crate::smartcontract::{self, ExtendedType, FieldExtendedType};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extended_type_equals() {
        let crazy_t = ExtendedType {
            base: smartcontract::Type::String,
            name: Some("qwertyu".to_string()),
            interface: Some("qwerty".to_string()),
            key: Some(Box::new(smartcontract::Type::Bool)),
            value: Some(Box::new(ExtendedType {
                base: smartcontract::Type::Integer,
                ..Default::default()
            })),
            fields: vec![
                FieldExtendedType {
                    field: "qwe".to_string(),
                    extended_type: ExtendedType {
                        base: smartcontract::Type::Integer,
                        name: Some("qwer".to_string()),
                        interface: Some("qw".to_string()),
                        key: Some(Box::new(smartcontract::Type::Array)),
                        fields: vec![
                            FieldExtendedType {
                                field: "as".to_string(),
                                extended_type: ExtendedType::default(),
                            },
                        ],
                        ..Default::default()
                    },
                },
                FieldExtendedType {
                    field: "asf".to_string(),
                    extended_type: ExtendedType {
                        base: smartcontract::Type::Bool,
                        ..Default::default()
                    },
                },
                FieldExtendedType {
                    field: "sffg".to_string(),
                    extended_type: ExtendedType {
                        base: smartcontract::Type::Any,
                        ..Default::default()
                    },
                },
            ],
        };

        let test_cases = vec![
            ("both nil", None, None, true),
            ("a is nil", None, Some(ExtendedType::default()), false),
            ("b is nil", Some(ExtendedType::default()), None, false),
            (
                "base mismatch",
                Some(ExtendedType { base: smartcontract::Type::String, ..Default::default() }),
                Some(ExtendedType { base: smartcontract::Type::Integer, ..Default::default() }),
                false,
            ),
            (
                "name mismatch",
                Some(ExtendedType { base: smartcontract::Type::Array, name: Some("q".to_string()), ..Default::default() }),
                Some(ExtendedType { base: smartcontract::Type::Array, name: Some("w".to_string()), ..Default::default() }),
                false,
            ),
            (
                "number of fields mismatch",
                Some(ExtendedType {
                    base: smartcontract::Type::Array,
                    name: Some("q".to_string()),
                    fields: vec![
                        FieldExtendedType {
                            field: "IntField".to_string(),
                            extended_type: ExtendedType { base: smartcontract::Type::Integer, ..Default::default() },
                        },
                    ],
                    ..Default::default()
                }),
                Some(ExtendedType {
                    base: smartcontract::Type::Array,
                    name: Some("w".to_string()),
                    fields: vec![
                        FieldExtendedType {
                            field: "IntField".to_string(),
                            extended_type: ExtendedType { base: smartcontract::Type::Integer, ..Default::default() },
                        },
                        FieldExtendedType {
                            field: "BoolField".to_string(),
                            extended_type: ExtendedType { base: smartcontract::Type::Bool, ..Default::default() },
                        },
                    ],
                    ..Default::default()
                }),
                false,
            ),
            (
                "field names mismatch",
                Some(ExtendedType {
                    base: smartcontract::Type::Array,
                    fields: vec![
                        FieldExtendedType {
                            field: "IntField".to_string(),
                            extended_type: ExtendedType { base: smartcontract::Type::Integer, ..Default::default() },
                        },
                    ],
                    ..Default::default()
                }),
                Some(ExtendedType {
                    base: smartcontract::Type::Array,
                    fields: vec![
                        FieldExtendedType {
                            field: "BoolField".to_string(),
                            extended_type: ExtendedType { base: smartcontract::Type::Bool, ..Default::default() },
                        },
                    ],
                    ..Default::default()
                }),
                false,
            ),
            (
                "field types mismatch",
                Some(ExtendedType {
                    base: smartcontract::Type::Array,
                    fields: vec![
                        FieldExtendedType {
                            field: "Field".to_string(),
                            extended_type: ExtendedType { base: smartcontract::Type::Integer, ..Default::default() },
                        },
                    ],
                    ..Default::default()
                }),
                Some(ExtendedType {
                    base: smartcontract::Type::Array,
                    fields: vec![
                        FieldExtendedType {
                            field: "Field".to_string(),
                            extended_type: ExtendedType { base: smartcontract::Type::Bool, ..Default::default() },
                        },
                    ],
                    ..Default::default()
                }),
                false,
            ),
            (
                "interface mismatch",
                Some(ExtendedType { interface: Some("iterator".to_string()), ..Default::default() }),
                Some(ExtendedType { interface: Some("unknown".to_string()), ..Default::default() }),
                false,
            ),
            (
                "value is nil",
                Some(ExtendedType { base: smartcontract::Type::String, ..Default::default() }),
                Some(ExtendedType { base: smartcontract::Type::String, ..Default::default() }),
                true,
            ),
            (
                "a value is not nil",
                Some(ExtendedType {
                    base: smartcontract::Type::Array,
                    value: Some(Box::new(ExtendedType::default())),
                    ..Default::default()
                }),
                Some(ExtendedType { base: smartcontract::Type::Array, ..Default::default() }),
                false,
            ),
            (
                "b value is not nil",
                Some(ExtendedType { base: smartcontract::Type::Array, ..Default::default() }),
                Some(ExtendedType {
                    base: smartcontract::Type::Array,
                    value: Some(Box::new(ExtendedType::default())),
                    ..Default::default()
                }),
                false,
            ),
            (
                "byte array tolerance for a",
                Some(ExtendedType { base: smartcontract::Type::String, ..Default::default() }),
                Some(ExtendedType { base: smartcontract::Type::ByteArray, ..Default::default() }),
                true,
            ),
            (
                "byte array tolerance for b",
                Some(ExtendedType { base: smartcontract::Type::ByteArray, ..Default::default() }),
                Some(ExtendedType { base: smartcontract::Type::String, ..Default::default() }),
                true,
            ),
            (
                "key mismatch",
                Some(ExtendedType { key: Some(Box::new(smartcontract::Type::String)), ..Default::default() }),
                Some(ExtendedType { key: Some(Box::new(smartcontract::Type::Integer)), ..Default::default() }),
                false,
            ),
            ("good nested", Some(crazy_t.clone()), Some(crazy_t), true),
        ];

        for (name, a, b, expected_res) in test_cases {
            assert_eq!(a.as_ref().map(|a| a.equals(b.as_ref())).unwrap_or(b.is_none()), expected_res, "{}", name);
        }
    }
}
