use serde_json::Value;
use std::str::FromStr;
use std::fmt;
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::collections::HashMap;
use num_bigint::BigInt;
use uuid::Uuid;
use base64;
use hex;
use serde_json::json;

#[derive(Debug, PartialEq)]
struct Param {
    raw_message: Value,
}

impl Param {
    fn get_string_strict(&self) -> Result<String, &str> {
        if let Value::String(ref s) = self.raw_message {
            Ok(s.clone())
        } else {
            Err("Not a string")
        }
    }

    fn get_string(&self) -> Result<String, &str> {
        match &self.raw_message {
            Value::String(s) => Ok(s.clone()),
            Value::Number(n) => Ok(n.to_string()),
            _ => Err("Not a string or number"),
        }
    }

    fn get_int_strict(&self) -> Result<i64, &str> {
        if let Value::Number(ref n) = self.raw_message {
            n.as_i64().ok_or("Not an integer")
        } else {
            Err("Not an integer")
        }
    }

    fn get_int(&self) -> Result<i64, &str> {
        match &self.raw_message {
            Value::Number(n) => n.as_i64().ok_or("Not an integer"),
            Value::String(s) => s.parse::<i64>().map_err(|_| "Not an integer"),
            _ => Err("Not an integer"),
        }
    }

    fn get_boolean_strict(&self) -> Result<bool, &str> {
        if let Value::Bool(b) = self.raw_message {
            Ok(*b)
        } else {
            Err("Not a boolean")
        }
    }

    fn get_boolean(&self) -> Result<bool, &str> {
        match &self.raw_message {
            Value::Bool(b) => Ok(*b),
            Value::String(s) => s.parse::<bool>().map_err(|_| "Not a boolean"),
            _ => Err("Not a boolean"),
        }
    }

    fn get_array(&self) -> Result<Vec<Param>, &str> {
        if let Value::Array(arr) = &self.raw_message {
            Ok(arr.iter().map(|v| Param { raw_message: v.clone() }).collect())
        } else {
            Err("Not an array")
        }
    }

    fn get_big_int(&self) -> Result<BigInt, &str> {
        match &self.raw_message {
            Value::String(s) => BigInt::from_str(s).map_err(|_| "Not a big integer"),
            Value::Number(n) => BigInt::from_str(&n.to_string()).map_err(|_| "Not a big integer"),
            Value::Bool(b) => Ok(BigInt::from(*b as i64)),
            _ => Err("Not a big integer"),
        }
    }

    fn get_uuid(&self) -> Result<Uuid, &str> {
        if let Value::String(s) = &self.raw_message {
            Uuid::parse_str(s).map_err(|_| "Not a valid UUID")
        } else {
            Err("Not a string")
        }
    }

    fn get_bytes_hex(&self) -> Result<Vec<u8>, &str> {
        if let Value::String(s) = &self.raw_message {
            hex::decode(s).map_err(|_| "Not a valid hex string")
        } else {
            Err("Not a string")
        }
    }

    fn get_bytes_base64(&self) -> Result<Vec<u8>, &str> {
        if let Value::String(s) = &self.raw_message {
            base64::decode(s).map_err(|_| "Not a valid base64 string")
        } else {
            Err("Not a string")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_param_unmarshal_json() {
        let msg = r#"["123", 123, null, ["str2", 3], [{"type": "String", "value": "jajaja"}],
            {"account": "0xcadb3dc2faa3ef14a13b619c9a43124755aa2569"},
            {"account": "NYxb4fSZVKAz8YsgaPK2WkT3KcAE9b3Vag", "scopes": "Global"},
            [{"account": "0xcadb3dc2faa3ef14a13b619c9a43124755aa2569", "scopes": "Global"}]]"#;

        let expected = vec![
            json!("123"),
            json!(123),
            json!(null),
            json!(["str2", 3]),
            json!([{"type": "String", "value": "jajaja"}]),
            json!({"account": "0xcadb3dc2faa3ef14a13b619c9a43124755aa2569"}),
            json!({"account": "NYxb4fSZVKAz8YsgaPK2WkT3KcAE9b3Vag", "scopes": "Global"}),
            json!([{"account": "0xcadb3dc2faa3ef14a13b619c9a43124755aa2569", "scopes": "Global"}]),
        ];

        let ps: Vec<Param> = serde_json::from_str(msg).unwrap();
        assert_eq!(ps.len(), expected.len());

        for (i, tc) in expected.iter().enumerate() {
            assert_eq!(ps[i].raw_message, *tc);
        }
    }

    #[test]
    fn test_get_big_int() {
        let test_cases = vec![
            ("true", BigInt::from(1)),
            ("false", BigInt::from(0)),
            ("42", BigInt::from(42)),
            ("-9223372036854775808", BigInt::from(-9223372036854775808i64)),
            ("18446744073709551615", BigInt::from(18446744073709551615u64)),
            ("-9223372036854775808000", BigInt::from(-9223372036854775808000i64)),
            ("18446744073709551615000", BigInt::from(18446744073709551615000u64)),
            ("abc", BigInt::from(0)),
            ("[]", BigInt::from(0)),
            ("null", BigInt::from(0)),
        ];

        for (raw, expected) in test_cases {
            let p = Param { raw_message: serde_json::from_str(raw).unwrap() };
            let actual = p.get_big_int().unwrap_or(BigInt::from(0));
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn test_get_uuid() {
        let test_cases = vec![
            ("null", false),
            ("\"not-a-uuid\"", false),
            ("\"2107da59-4f9c-462c-9c51-7666842519a9\"", true),
        ];

        for (raw, should_pass) in test_cases {
            let p = Param { raw_message: serde_json::from_str(raw).unwrap() };
            let result = p.get_uuid();
            assert_eq!(result.is_ok(), should_pass);
        }
    }

    #[test]
    fn test_get_bytes_hex() {
        let test_cases = vec![
            ("\"602c79718b16e442de58778e148d0b1084e3b2dffd5de6b7b16cee7969282de7\"", true),
            ("42", true),
            ("\"qq2c79718b16e442de58778e148d0b1084e3b2dffd5de6b7b16cee7969282de7\"", false),
        ];

        for (raw, should_pass) in test_cases {
            let p = Param { raw_message: serde_json::from_str(raw).unwrap() };
            let result = p.get_bytes_hex();
            assert_eq!(result.is_ok(), should_pass);
        }
    }

    #[test]
    fn test_get_bytes_base64() {
        let test_cases = vec![
            ("\"Aj4A8DoW6HB84EXrQu6A05JFFUHuUQ3BjhyL77rFTXQm\"", true),
            ("42", false),
            ("\"@j4A8DoW6HB84EXrQu6A05JFFUHuUQ3BjhyL77rFTXQm\"", false),
        ];

        for (raw, should_pass) in test_cases {
            let p = Param { raw_message: serde_json::from_str(raw).unwrap() };
            let result = p.get_bytes_base64();
            assert_eq!(result.is_ok(), should_pass);
        }
    }
}
