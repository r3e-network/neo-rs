use std::fmt;
use std::panic;
use std::any::Any;

use crate::io::{BinReader, BinWriter, GetVarSize};
use crate::util;
use assert2::assert;

#[derive(Default)]
struct SmthSerializable {
    some: [u8; 42],
}

impl SmthSerializable {
    fn decode_binary(&self, _br: &mut BinReader) {}

    fn encode_binary(&self, bw: &mut BinWriter) {
        bw.write_bytes(&self.some);
    }
}

#[derive(Default)]
struct SmthNotReallySerializable;

impl SmthNotReallySerializable {
    fn decode_binary(&self, _br: &mut BinReader) {}

    fn encode_binary(&self, bw: &mut BinWriter) {
        bw.err = Some(fmt::Error);
    }
}

#[test]
fn test_var_size() {
    struct TestCase {
        variable: Box<dyn Any>,
        name: &'static str,
        expected: usize,
    }

    let test_cases = vec![
        TestCase {
            variable: Box::new(252),
            name: "test_int_1",
            expected: 1,
        },
        TestCase {
            variable: Box::new(253),
            name: "test_int_2",
            expected: 3,
        },
        TestCase {
            variable: Box::new(65535),
            name: "test_int_3",
            expected: 3,
        },
        TestCase {
            variable: Box::new(65536),
            name: "test_int_4",
            expected: 5,
        },
        TestCase {
            variable: Box::new(4294967295u32),
            name: "test_int_5",
            expected: 5,
        },
        TestCase {
            variable: Box::new(252u32),
            name: "test_uint_1",
            expected: 1,
        },
        TestCase {
            variable: Box::new(253u32),
            name: "test_uint_2",
            expected: 3,
        },
        TestCase {
            variable: Box::new(65535u32),
            name: "test_uint_3",
            expected: 3,
        },
        TestCase {
            variable: Box::new(65536u32),
            name: "test_uint_4",
            expected: 5,
        },
        TestCase {
            variable: Box::new(4294967295u32),
            name: "test_uint_5",
            expected: 5,
        },
        TestCase {
            variable: Box::new(vec![1, 2, 4, 5, 6]),
            name: "test_[]byte_1",
            expected: 6,
        },
        TestCase {
            variable: Box::new(util::Uint160::from([1, 2, 4, 5, 6])),
            name: "test_Uint160_1",
            expected: 21,
        },
        TestCase {
            variable: Box::new([1u8, 2, 3, 4, 5, 6]),
            name: "test_uint8_1",
            expected: 21,
        },
        TestCase {
            variable: Box::new([1u8, 2, 3, 4, 5, 6, 8, 9]),
            name: "test_uint8_2",
            expected: 21,
        },
        TestCase {
            variable: Box::new([1u8, 2, 3, 4, 5, 6]),
            name: "test_uint8_3",
            expected: 33,
        },
        TestCase {
            variable: Box::new([1u16, 2, 3, 4, 5, 6]),
            name: "test_uint16_1",
            expected: 21,
        },
        TestCase {
            variable: Box::new([1u16, 2, 3, 4, 5, 6, 10, 21]),
            name: "test_uint16_2",
            expected: 21,
        },
        TestCase {
            variable: Box::new([1u32, 2, 3, 4, 5, 6, 10, 21]),
            name: "test_uint32_2",
            expected: 121,
        },
        TestCase {
            variable: Box::new([1u64, 2, 3, 4, 5, 6, 10, 21]),
            name: "test_uint64_2",
            expected: 241,
        },
        TestCase {
            variable: Box::new([1i8, 2, 3, 4, 5, 6]),
            name: "test_int8_1",
            expected: 21,
        },
        TestCase {
            variable: Box::new([-1i8, 2, 3, 4, 5, 6, 8, 9]),
            name: "test_int8_2",
            expected: 21,
        },
        TestCase {
            variable: Box::new([-1i8, 2, 3, 4, 5, 6]),
            name: "test_int8_3",
            expected: 33,
        },
        TestCase {
            variable: Box::new([-1i16, 2, 3, 4, 5, 6]),
            name: "test_int16_1",
            expected: 21,
        },
        TestCase {
            variable: Box::new([-1i16, 2, 3, 4, 5, 6, 10, 21]),
            name: "test_int16_2",
            expected: 21,
        },
        TestCase {
            variable: Box::new([-1i32, 2, 3, 4, 5, 6, 10, 21]),
            name: "test_int32_2",
            expected: 121,
        },
        TestCase {
            variable: Box::new([-1i64, 2, 3, 4, 5, 6, 10, 21]),
            name: "test_int64_2",
            expected: 241,
        },
        TestCase {
            variable: Box::new(util::Uint256::from([1, 2, 3, 4, 5, 6])),
            name: "test_Uint256_1",
            expected: 33,
        },
        TestCase {
            variable: Box::new("abc".to_string()),
            name: "test_string_1",
            expected: 4,
        },
        TestCase {
            variable: Box::new("abcà".to_string()),
            name: "test_string_2",
            expected: 6,
        },
        TestCase {
            variable: Box::new("2d3b96ae1bcc5a585e075e3b81920210dec16302".to_string()),
            name: "test_string_3",
            expected: 41,
        },
        TestCase {
            variable: Box::new(vec![SmthSerializable::default(), SmthSerializable::default()]),
            name: "test_Serializable",
            expected: 2 * 42 + 1,
        },
    ];

    for tc in test_cases {
        let result = GetVarSize::get_var_size(&*tc.variable);
        assert!(result == tc.expected);
    }
}

fn panic_var_size(v: Box<dyn Any>) {
    let result = panic::catch_unwind(|| {
        GetVarSize::get_var_size(&*v);
    });
    assert!(result.is_err());
}

#[test]
fn test_var_size_panic() {
    panic_var_size(Box::new(()));
    panic_var_size(Box::new(SmthNotReallySerializable::default()));
}
