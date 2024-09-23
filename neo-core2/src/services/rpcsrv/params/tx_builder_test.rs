use std::str::FromStr;
use std::fmt;
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::num::ParseIntError;
use std::string::FromUtf8Error;
use std::str::Utf8Error;
use std::string::ToString;
use base64;
use bigdecimal::BigDecimal;
use serde_json::json;
use crate::io;
use crate::util;
use crate::vm::opcode;
use crate::smartcontract::Parameter;
use crate::transaction::{self, Signer};
use crate::neorpc;
use anyhow::Result;
use serde_json::Value as JsonValue;

#[derive(Serialize, Deserialize, Debug)]
pub struct Param {
    #[serde(flatten)]
    raw_message: JsonValue,
}

pub type Params = Vec<Param>;

impl Param {
    pub fn get_uint160_from_hex(&self) -> Result<util::Uint160> {
        let hex_str = self.raw_message.as_str().ok_or_else(|| anyhow!("Invalid hex string"))?;
        util::Uint160::from_str(hex_str).map_err(|e| anyhow!(e.to_string()))
    }

    pub fn get_string(&self) -> Result<String> {
        self.raw_message.as_str().map(|s| s.to_string()).ok_or_else(|| anyhow!("Invalid string"))
    }
}

#[test]
fn test_invocation_script_creation_good() -> Result<()> {
    let p = Param { raw_message: json!("50befd26fdf6e4d957c11e078b24ebce6291456f") };
    let contract = p.get_uint160_from_hex()?;

    let param_scripts = vec![
        (vec![Param { raw_message: json!("transfer") }], "wh8MCHRyYW5zZmVyDBRvRZFizuskiwcewVfZ5Pb9Jv2+UEFifVtS"),
        (vec![Param { raw_message: json!(42) }], "wh8MAjQyDBRvRZFizuskiwcewVfZ5Pb9Jv2+UEFifVtS"),
        (vec![Param { raw_message: json!("a") }, Param { raw_message: json!([]) }], "wh8MAWEMFG9FkWLO6ySLBx7BV9nk9v0m/b5QQWJ9W1I="),
        (vec![Param { raw_message: json!("a") }, Param { raw_message: json!([{"type": "ByteArray", "value": "AwEtR+diEK7HO+Oas9GG4KQP6Nhr+j1Pq/2le6E7iPlq"}]) }], "DCEDAS1H52IQrsc745qz0YbgpA/o2Gv6PU+r/aV7oTuI+WoRwB8MAWEMFG9FkWLO6ySLBx7BV9nk9v0m/b5QQWJ9W1I="),
        (vec![Param { raw_message: json!("a") }, Param { raw_message: json!([{"type": "Signature", "value": "4edf5005771de04619235d5a4c7a9a11bb78e008541f1da7725f654c33380a3c87e2959a025da706d7255cb3a3fa07ebe9c6559d0d9e6213c68049168eb1056f"}]) }], "DGDh51/nTTnvvV17TjrX3bfl3lrhztr1rXVtvvx7TTznjV/V1rvvbl/rnhzfffzRrdzzt7b3n1rTbl1rvTp3vbnlxvdrd9rTt5t71zrnn13R317rbXdzrzTj3Xrx5vXTnp8RwB8MAWEMFG9FkWLO6ySLBx7BV9nk9v0m/b5QQWJ9W1I="),
        (vec![Param { raw_message: json!("a") }, Param { raw_message: json!([{"type": "Signature", "value": "Tt9QBXcd4EYZI11aTHqaEbt44AhUHx2ncl9lTDM4CjyH4pWaAl2nBtclXLOj+gfr6cZVnQ2eYhPGgEkWjrEFbw=="}]) }], "DEBO31AFdx3gRhkjXVpMepoRu3jgCFQfHadyX2VMMzgKPIfilZoCXacG1yVcs6P6B+vpxlWdDZ5iE8aASRaOsQVvEcAfDAFhDBRvRZFizuskiwcewVfZ5Pb9Jv2+UEFifVtS"),
        (vec![Param { raw_message: json!("a") }, Param { raw_message: json!([{"type": "String", "value": "50befd26fdf6e4d957c11e078b24ebce6291456f"}]) }], "DCg1MGJlZmQyNmZkZjZlNGQ5NTdjMTFlMDc4YjI0ZWJjZTYyOTE0NTZmEcAfDAFhDBRvRZFizuskiwcewVfZ5Pb9Jv2+UEFifVtS"),
        (vec![Param { raw_message: json!("a") }, Param { raw_message: json!([{"type": "Hash160", "value": "50befd26fdf6e4d957c11e078b24ebce6291456f"}]) }], "DBRvRZFizuskiwcewVfZ5Pb9Jv2+UBHAHwwBYQwUb0WRYs7rJIsHHsFX2eT2/Sb9vlBBYn1bUg=="),
        (vec![Param { raw_message: json!("a") }, Param { raw_message: json!([{"type": "Hash256", "value": "602c79718b16e442de58778e148d0b1084e3b2dffd5de6b7b16cee7969282de7"}]) }], "DCDnLShpee5ssbfmXf3fsuOEEAuNFI53WN5C5BaLcXksYBHAHwwBYQwUb0WRYs7rJIsHHsFX2eT2/Sb9vlBBYn1bUg=="),
        (vec![Param { raw_message: json!("a") }, Param { raw_message: json!([{"type": "PublicKey", "value": "03c089d7122b840a4935234e82e26ae5efd0c2acb627239dc9f207311337b6f2c1"}]) }], "DCEDwInXEiuECkk1I06C4mrl79DCrLYnI53J8gcxEze28sERwB8MAWEMFG9FkWLO6ySLBx7BV9nk9v0m/b5QQWJ9W1I="),
        (vec![Param { raw_message: json!("a") }, Param { raw_message: json!([{"type": "Integer", "value": 42}]) }], "ACoRwB8MAWEMFG9FkWLO6ySLBx7BV9nk9v0m/b5QQWJ9W1I="),
        (vec![Param { raw_message: json!("a") }, Param { raw_message: json!([{"type": "Integer", "value": "42"}]) }], "ACoRwB8MAWEMFG9FkWLO6ySLBx7BV9nk9v0m/b5QQWJ9W1I="),
        (vec![Param { raw_message: json!("a") }, Param { raw_message: json!([{"type": "Integer", "value": true}]) }], "ERHAHwwBYQwUb0WRYs7rJIsHHsFX2eT2/Sb9vlBBYn1bUg=="),
        (vec![Param { raw_message: json!("a") }, Param { raw_message: json!([{"type": "Boolean", "value": true}]) }], "CBHAHwwBYQwUb0WRYs7rJIsHHsFX2eT2/Sb9vlBBYn1bUg=="),
        (vec![Param { raw_message: json!("a") }, Param { raw_message: json!([{"type": "Boolean", "value": false}]) }], "CRHAHwwBYQwUb0WRYs7rJIsHHsFX2eT2/Sb9vlBBYn1bUg=="),
        (vec![Param { raw_message: json!("a") }, Param { raw_message: json!([{"type": "Boolean", "value": "blah"}]) }], "CBHAHwwBYQwUb0WRYs7rJIsHHsFX2eT2/Sb9vlBBYn1bUg=="),
    ];

    for (i, (ps, expected_script)) in param_scripts.iter().enumerate() {
        let method = ps[0].get_string()?;
        let p = if ps.len() > 1 { Some(&ps[1]) } else { None };
        let script = create_function_invocation_script(&contract, &method, p)?;
        assert_eq!(expected_script, &base64::encode(script), "testcase #{}", i);
    }

    Ok(())
}

#[test]
fn test_invocation_script_creation_bad() -> Result<()> {
    let contract = util::Uint160::default();

    let test_params = vec![
        Param { raw_message: json!(true) },
        Param { raw_message: json!([{"type": "ByteArray", "value": "qwerty"}]) },
        Param { raw_message: json!([{"type": "Signature", "value": "qwerty"}]) },
        Param { raw_message: json!([{"type": "Hash160", "value": "qwerty"}]) },
        Param { raw_message: json!([{"type": "Hash256", "value": "qwerty"}]) },
        Param { raw_message: json!([{"type": "PublicKey", "value": 42}]) },
        Param { raw_message: json!([{"type": "PublicKey", "value": "qwerty"}]) },
        Param { raw_message: json!([{"type": "Integer", "value": "123q"}]) },
        Param { raw_message: json!([{"type": "Unknown"}]) },
    ];

    for (i, ps) in test_params.iter().enumerate() {
        let result = create_function_invocation_script(&contract, "", Some(ps));
        assert!(result.is_err(), "testcase #{}", i);
    }

    Ok(())
}

#[test]
fn test_expand_array_into_script() -> Result<()> {
    let bi = BigDecimal::from(1u64) << 254;
    let raw_int = vec![0; 31].into_iter().chain(vec![0x40]).collect::<Vec<u8>>();

    let test_cases = vec![
        (vec![Param { raw_message: json!({"type": "String", "value": "a"}) }], vec![opcode::PUSHDATA1, 1, b'a']),
        (vec![Param { raw_message: json!({"type": "Array", "value": [{"type": "String", "value": "a"}]}) }], vec![opcode::PUSHDATA1, 1, b'a', opcode::PUSH1, opcode::PACK]),
        (vec![Param { raw_message: json!({"type": "Integer", "value": bi.to_string()}) }], [vec![opcode::PUSHINT256], raw_int.clone()].concat()),
        (vec![Param { raw_message: json!({"type": "Map", "value": [{"key": {"type": "String", "value": "a"}, "value": {"type": "Integer", "value": 1}}, {"key": {"type": "String", "value": "b"}, "value": {"type": "Integer", "value": 2}}]}) }], vec![opcode::PUSH2, opcode::PUSHDATA1, 1, b'b', opcode::PUSH1, opcode::PUSHDATA1, 1, b'a', opcode::PUSH2, opcode::PACKMAP]),
    ];

    for (input, expected) in test_cases {
        let mut script = io::BufBinWriter::new();
        expand_array_into_script(&mut script, &input)?;
        assert_eq!(expected, script.bytes());
    }

    let error_cases = vec![
        vec![Param { raw_message: json!({"type": "Array", "value": "a"}) }],
        vec![Param { raw_message: json!({"type": "Array", "value": null}) }],
        vec![Param { raw_message: json!({"type": "Integer", "value": (BigDecimal::from(1u64) << 255).to_string()}) }],
        vec![Param { raw_message: json!({"type": "Map", "value": [{"key": {"type": "InvalidT", "value": "a"}, "value": {"type": "Integer", "value": 1}}]}) }],
        vec![Param { raw_message: json!({"type": "Map", "value": [{"key": {"type": "String", "value": "a"}, "value": {"type": "Integer", "value": "not-an-int"}}]}) }],
    ];

    for c in error_cases {
        let mut script = io::BufBinWriter::new();
        let result = expand_array_into_script(&mut script, &c);
        assert!(result.is_err());
    }

    Ok(())
}
