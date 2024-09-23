use base64;
use serde_json;
use std::convert::TryInto;

use crate::internal::random;
use crate::internal::testserdes;
use crate::pkg::io;
use crate::pkg::smartcontract::callflag;
use crate::pkg::util;
use crate::pkg::vm::stackitem;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_binary() {
        let script = vec![12, 32, 84, 35, 14];
        let expected = File {
            header: Header {
                magic: MAGIC,
                compiler: "best compiler version 1".to_string(),
            },
            tokens: vec![MethodToken {
                hash: random::uint160(),
                method: "method".to_string(),
                param_count: 3,
                has_return: true,
                call_flag: callflag::WriteStates,
            }],
            script: script.clone(),
            checksum: 0, // Will be calculated later
        };

        // Invalid Magic
        {
            let mut invalid = expected.clone();
            invalid.header.magic = 123;
            check_decode_error(&invalid);
        }

        // Invalid checksum
        {
            let mut invalid = expected.clone();
            invalid.checksum = 123;
            check_decode_error(&invalid);
        }

        // Zero-length script
        {
            let mut invalid = expected.clone();
            invalid.script = vec![];
            invalid.checksum = invalid.calculate_checksum();
            check_decode_error(&invalid);
        }

        // Invalid script length
        {
            let mut invalid = expected.clone();
            invalid.script = vec![0; stackitem::MAX_SIZE + 1];
            invalid.checksum = invalid.calculate_checksum();
            check_decode_error(&invalid);
        }

        // Invalid tokens list
        {
            let mut invalid = expected.clone();
            invalid.tokens[0].method = "_reserved".to_string();
            invalid.checksum = invalid.calculate_checksum();
            check_decode_error(&invalid);
        }

        // Positive test
        {
            let mut valid = expected.clone();
            valid.checksum = valid.calculate_checksum();
            testserdes::encode_decode_binary(&valid, File::default());
        }

        // Positive test with empty tokens
        {
            let mut valid = expected.clone();
            valid.tokens = vec![];
            valid.checksum = valid.calculate_checksum();
            testserdes::encode_decode_binary(&valid, File::default());
        }

        // Invalid reserved bytes
        {
            let mut valid = expected.clone();
            valid.tokens.clear();
            valid.checksum = valid.calculate_checksum();
            let mut bytes = testserdes::encode_binary(&valid).unwrap();

            let sz = io::get_var_size(&valid.header);
            bytes[sz] = 1;
            assert!(testserdes::decode_binary::<File>(&bytes).is_err());

            bytes[sz] = 0;
            bytes[sz + 3] = 1;
            assert!(testserdes::decode_binary::<File>(&bytes).is_err());
        }
    }

    fn check_decode_error(file: &File) {
        let bytes = testserdes::encode_binary(file).unwrap();
        assert!(testserdes::decode_binary::<File>(&bytes).is_err());
    }

    #[test]
    fn test_bytes_from_bytes() {
        let script = vec![12, 32, 84, 35, 14];
        let expected = File {
            header: Header {
                magic: MAGIC,
                compiler: "best compiler version 1".to_string(),
            },
            tokens: vec![MethodToken {
                hash: random::uint160(),
                method: "someMethod".to_string(),
                param_count: 3,
                has_return: true,
                call_flag: callflag::WriteStates,
            }],
            script: script.clone(),
            checksum: 0,
        };
        let expected = {
            let mut e = expected;
            e.checksum = e.calculate_checksum();
            e
        };

        let bytes = expected.bytes().unwrap();
        let actual = File::from_bytes(&bytes).unwrap();
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_new_file_from_bytes_limits() {
        let expected = File {
            header: Header {
                magic: MAGIC,
                compiler: "best compiler version 1".to_string(),
            },
            tokens: vec![MethodToken {
                hash: random::uint160(),
                method: "someMethod".to_string(),
                param_count: 3,
                has_return: true,
                call_flag: callflag::WriteStates,
            }],
            script: vec![0; stackitem::MAX_SIZE - 100],
            checksum: 0,
        };
        let expected = {
            let mut e = expected;
            e.checksum = e.calculate_checksum();
            e
        };

        let bytes = expected.bytes_long().unwrap();
        assert!(File::from_bytes(&bytes).is_err());
    }

    #[test]
    fn test_marshal_unmarshal_json() {
        let expected = File {
            header: Header {
                magic: MAGIC,
                compiler: "test.compiler-test.ver".to_string(),
            },
            tokens: vec![MethodToken {
                hash: util::Uint160([0x12, 0x34, 0x56, 0x78, 0x91, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
                method: "someMethod".to_string(),
                param_count: 3,
                has_return: true,
                call_flag: callflag::WriteStates,
            }],
            script: vec![1, 2, 3, 4],
            checksum: 0,
        };
        let expected = {
            let mut e = expected;
            e.checksum = e.calculate_checksum();
            e
        };

        let data = serde_json::to_string(&expected).unwrap();
        let expected_json = format!(
            r#"{{
                "magic": {},
                "compiler": "test.compiler-test.ver",
                "source": "",
                "tokens": [
                    {{
                        "hash": "0x{}",
                        "method": "someMethod",
                        "paramcount": 3,
                        "hasreturnvalue": true,
                        "callflags": "{}"
                    }}
                ],
                "script": "{}",
                "checksum": {}
            }}"#,
            MAGIC,
            expected.tokens[0].hash.to_string_le(),
            expected.tokens[0].call_flag.to_string(),
            base64::encode(&expected.script),
            expected.checksum
        );

        assert_eq!(serde_json::from_str::<serde_json::Value>(&data).unwrap(), serde_json::from_str::<serde_json::Value>(&expected_json).unwrap());

        let actual: File = serde_json::from_str(&data).unwrap();
        assert_eq!(expected, actual);
    }
}
