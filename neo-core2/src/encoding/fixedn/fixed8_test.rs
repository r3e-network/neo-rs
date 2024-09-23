use serde::{Deserialize, Serialize};
use serde_json;
use std::str::FromStr;
use std::f64;
use std::i64;
use std::fmt;
use std::cmp::Ordering;
use std::ops::{Add, Sub, Div};
use yaml_rust::{YamlLoader, YamlEmitter};
use std::error::Error;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Fixed8(i64);

impl Fixed8 {
    fn from_int64(val: i64) -> Self {
        Fixed8(val * 100_000_000)
    }

    fn integral_value(&self) -> i64 {
        self.0 / 100_000_000
    }

    fn fractional_value(&self) -> i32 {
        (self.0 % 100_000_000) as i32
    }

    fn from_float(val: f64) -> Self {
        Fixed8((val * 100_000_000.0).round() as i64)
    }

    fn float_value(&self) -> f64 {
        self.0 as f64 / 100_000_000.0
    }

    fn from_string(val: &str) -> Result<Self, Box<dyn Error>> {
        let parsed = f64::from_str(val)?;
        Ok(Fixed8::from_float(parsed))
    }

    fn add(&self, other: Fixed8) -> Fixed8 {
        Fixed8(self.0 + other.0)
    }

    fn sub(&self, other: Fixed8) -> Fixed8 {
        Fixed8(self.0 - other.0)
    }

    fn less_than(&self, other: Fixed8) -> bool {
        self.0 < other.0
    }

    fn greater_than(&self, other: Fixed8) -> bool {
        self.0 > other.0
    }

    fn equal(&self, other: Fixed8) -> bool {
        self.0 == other.0
    }

    fn compare(&self, other: Fixed8) -> Ordering {
        self.0.cmp(&other.0)
    }

    fn div(&self, divisor: i64) -> Fixed8 {
        Fixed8(self.0 / divisor)
    }
}

impl fmt::Display for Fixed8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.float_value())
    }
}

#[test]
fn test_fixed8_from_int64() {
    let values = vec![9000, 100000000, 5, 10945, -42];

    for val in values {
        assert_eq!(Fixed8::from_int64(val), Fixed8(val * 100_000_000));
        assert_eq!(val, Fixed8::from_int64(val).integral_value());
        assert_eq!(0, Fixed8::from_int64(val).fractional_value());
    }
}

#[test]
fn test_fixed8_add() {
    let a = Fixed8::from_int64(1);
    let b = Fixed8::from_int64(2);

    let c = a.add(b);
    let expected = 3;
    assert_eq!(expected.to_string(), c.to_string());
}

#[test]
fn test_fixed8_sub() {
    let a = Fixed8::from_int64(42);
    let b = Fixed8::from_int64(34);

    let c = a.sub(b);
    assert_eq!(8, c.integral_value());
    assert_eq!(0, c.fractional_value());
}

#[test]
fn test_fixed8_from_float() {
    let inputs = vec![12.98, 23.87654333, 100.654322, 456789.12345665, -3.14159265];

    for val in inputs {
        assert_eq!(Fixed8::from_float(val), Fixed8((val * 100_000_000.0).round() as i64));
        assert_eq!(val, Fixed8::from_float(val).float_value());
        let trunc = val.trunc();
        let rem = (val - trunc) * 100_000_000.0;
        assert_eq!(trunc as i64, Fixed8::from_float(val).integral_value());
        assert_eq!(rem.round() as i32, Fixed8::from_float(val).fractional_value());
    }
}

#[test]
fn test_fixed8_from_string() {
    let ivalues = vec!["9000", "100000000", "5", "10945", "20.45", "0.00000001", "-42"];
    for val in ivalues {
        let n = Fixed8::from_string(val).unwrap();
        assert_eq!(val, n.to_string());
    }

    let val = "123456789.12345678";
    let n = Fixed8::from_string(val).unwrap();
    assert_eq!(Fixed8(12345678912345678), n);

    let val = "901.2341";
    let n = Fixed8::from_string(val).unwrap();
    assert_eq!(Fixed8(90123410000), n);

    let val = "90n1";
    assert!(Fixed8::from_string(val).is_err());

    let val = "90.1s";
    assert!(Fixed8::from_string(val).is_err());
}

#[test]
fn test_satoshi() {
    let satoshif8 = Fixed8(1);
    assert_eq!("0.00000001", satoshif8.to_string());
}

#[test]
fn test_fixed8_unmarshal_json() {
    let test_cases = vec![123.45, -123.45];

    for fl in test_cases {
        let str = fl.to_string();
        let expected = Fixed8::from_string(&str).unwrap();

        let s = serde_json::to_string(&fl).unwrap();
        let u1: Fixed8 = serde_json::from_str(&s).unwrap();
        assert_eq!(expected, u1);

        let s = serde_json::to_string(&str).unwrap();
        let u2: Fixed8 = serde_json::from_str(&s).unwrap();
        assert_eq!(expected, u2);
    }

    let error_cases = vec!["\"123.u\"", "13.j"];

    for tc in error_cases {
        let result: Result<Fixed8, _> = serde_json::from_str(tc);
        assert!(result.is_err());
    }
}

#[test]
fn test_fixed8_unmarshal() {
    let expected = Fixed8(223719420);
    let cases = vec!["2.2371942", "\"2.2371942\""];

    for c in cases {
        let u1: Fixed8 = serde_json::from_str(c).unwrap();
        assert_eq!(expected, u1);
        let u2: Fixed8 = serde_yaml::from_str(c).unwrap();
        assert_eq!(expected, u2);
    }
}

#[test]
fn test_fixed8_marshal_json() {
    let u = Fixed8::from_string("123.4").unwrap();

    let s = serde_json::to_string(&u).unwrap();
    assert_eq!("\"123.4\"", s);
}

#[test]
fn test_fixed8_unmarshal_yaml() {
    let u = Fixed8::from_string("123.4").unwrap();

    let s = serde_yaml::to_string(&u).unwrap();
    assert_eq!("\"123.4\"\n", s);

    let f: Fixed8 = serde_yaml::from_str("\"123.4\"").unwrap();
    assert_eq!(u, f);
}

#[test]
fn test_fixed8_arith() {
    let u1 = Fixed8::from_int64(3);
    let u2 = Fixed8::from_int64(8);

    assert!(u1.less_than(u2));
    assert!(u2.greater_than(u1));
    assert!(u1.equal(u1));
    assert_ne!(u1.compare(u2), Ordering::Equal);
    assert_eq!(u1.compare(u1), Ordering::Equal);
    assert_eq!(Fixed8(2), u2.div(3));
}

#[test]
fn test_fixed8_serializable() {
    let a = Fixed8(0x0102030405060708);

    let encoded = bincode::serialize(&a).unwrap();
    let decoded: Fixed8 = bincode::deserialize(&encoded).unwrap();
    assert_eq!(a, decoded);
}
