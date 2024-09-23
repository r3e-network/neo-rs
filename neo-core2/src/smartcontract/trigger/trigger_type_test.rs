use crate::smartcontract::trigger::Type;
use std::convert::TryFrom;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stringer() {
        let tests = vec![
            (Type::OnPersist, "OnPersist"),
            (Type::PostPersist, "PostPersist"),
            (Type::Application, "Application"),
            (Type::Verification, "Verification"),
        ];
        for (trigger_type, expected_string) in tests {
            assert_eq!(expected_string, trigger_type.to_string());
        }
    }

    #[test]
    fn test_encode_binary() {
        let tests = vec![
            (Type::OnPersist, 0x01),
            (Type::PostPersist, 0x02),
            (Type::Verification, 0x20),
            (Type::Application, 0x40),
        ];
        for (trigger_type, expected_byte) in tests {
            assert_eq!(expected_byte, trigger_type as u8);
        }
    }

    #[test]
    fn test_decode_binary() {
        let tests = vec![
            (Type::OnPersist, 0x01),
            (Type::PostPersist, 0x02),
            (Type::Verification, 0x20),
            (Type::Application, 0x40),
        ];
        for (expected_type, byte) in tests {
            assert_eq!(expected_type, Type::try_from(byte).unwrap());
        }
    }

    #[test]
    fn test_from_string() {
        let test_cases = vec![
            ("OnPersist", Type::OnPersist),
            ("PostPersist", Type::PostPersist),
            ("Application", Type::Application),
            ("Verification", Type::Verification),
            ("All", Type::All),
        ];
        for (str_input, expected) in test_cases {
            let actual = Type::from_str(str_input).unwrap();
            assert_eq!(expected, actual);
        }

        let error_cases = vec!["", "Unknown"];
        for str_input in error_cases {
            assert!(Type::from_str(str_input).is_err());
        }
    }
}
