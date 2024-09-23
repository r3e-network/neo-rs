use num_bigint::BigInt;
use std::str::FromStr;
use std::string::ToString;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_decimal_from_string_good() {
        struct TestCase {
            bi: BigInt,
            prec: i32,
            s: &'static str,
        }

        let test_cases = vec![
            TestCase { bi: BigInt::from(123), prec: 2, s: "1.23" },
            TestCase { bi: BigInt::from(12300), prec: 2, s: "123" },
            TestCase { bi: BigInt::from(1234500000), prec: 8, s: "12.345" },
            TestCase { bi: BigInt::from(-12345), prec: 3, s: "-12.345" },
            TestCase { bi: BigInt::from(35), prec: 8, s: "0.00000035" },
            TestCase { bi: BigInt::from(1230), prec: 5, s: "0.0123" },
            TestCase { bi: BigInt::from(123456789), prec: 20, s: "0.00000000000123456789" },
        ];

        for tc in test_cases {
            let s = to_string(&tc.bi, tc.prec);
            assert_eq!(tc.s, s);

            let (bi, err) = from_string(s.as_str(), tc.prec);
            assert!(err.is_none());
            assert_eq!(tc.bi, bi.unwrap());
        }
    }

    #[test]
    fn test_decimal_from_string_bad() {
        struct ErrCase {
            s: &'static str,
            prec: i32,
        }

        let err_cases = vec![
            ErrCase { s: "", prec: 0 },
            ErrCase { s: "", prec: 1 },
            ErrCase { s: "12A", prec: 1 },
            ErrCase { s: "12.345", prec: 2 },
            ErrCase { s: "12.3A", prec: 2 },
        ];

        for tc in err_cases {
            let (_, err) = from_string(tc.s, tc.prec);
            assert!(err.is_some());
        }
    }

    fn to_string(bi: &BigInt, prec: i32) -> String {
        // Implement the conversion logic here
        unimplemented!()
    }

    fn from_string(s: &str, prec: i32) -> (Option<BigInt>, Option<String>) {
        // Implement the conversion logic here
        unimplemented!()
    }
}
