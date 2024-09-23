use std::collections::HashMap;
use std::str::FromStr;
use std::fmt;
use std::error::Error;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use uuid::Uuid;
use hex;
use base64;
use bigdecimal::BigDecimal;
use crate::core::transaction::{Signer, Witness};
use crate::encoding::address;
use crate::neorpc;
use crate::smartcontract;
use crate::util;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Param {
    #[serde(flatten)]
    raw_message: Value,
    cache: Option<Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FuncParam {
    #[serde(rename = "type")]
    param_type: smartcontract::ParamType,
    value: Param,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FuncParamKV {
    key: FuncParam,
    value: FuncParam,
}

impl Param {
    pub fn get_string_strict(&mut self) -> Result<String, Box<dyn Error>> {
        if self.raw_message.is_null() {
            return Err(Box::new(fmt::Error));
        }
        if self.cache.is_none() {
            let s: String = serde_json::from_value(self.raw_message.clone())?;
            self.cache = Some(Value::String(s.clone()));
            return Ok(s);
        }
        if let Some(Value::String(ref s)) = self.cache {
            return Ok(s.clone());
        }
        Err(Box::new(fmt::Error))
    }

    pub fn get_string(&mut self) -> Result<String, Box<dyn Error>> {
        if self.raw_message.is_null() {
            return Err(Box::new(fmt::Error));
        }
        if self.cache.is_none() {
            if let Ok(s) = serde_json::from_value::<String>(self.raw_message.clone()) {
                self.cache = Some(Value::String(s.clone()));
                return Ok(s);
            } else if let Ok(i) = serde_json::from_value::<i64>(self.raw_message.clone()) {
                self.cache = Some(Value::Number(i.into()));
                return Ok(i.to_string());
            } else if let Ok(b) = serde_json::from_value::<bool>(self.raw_message.clone()) {
                self.cache = Some(Value::Bool(b));
                return Ok(b.to_string());
            } else {
                return Err(Box::new(fmt::Error));
            }
        }
        match self.cache {
            Some(Value::String(ref s)) => Ok(s.clone()),
            Some(Value::Number(ref n)) => Ok(n.to_string()),
            Some(Value::Bool(b)) => Ok(b.to_string()),
            _ => Err(Box::new(fmt::Error)),
        }
    }

    pub fn get_boolean_strict(&mut self) -> Result<bool, Box<dyn Error>> {
        if self.raw_message.is_null() {
            return Err(Box::new(fmt::Error));
        }
        if self.raw_message == Value::Bool(true) {
            self.cache = Some(Value::Bool(true));
            return Ok(true);
        }
        if self.raw_message == Value::Bool(false) {
            self.cache = Some(Value::Bool(false));
            return Ok(false);
        }
        Err(Box::new(fmt::Error))
    }

    pub fn get_boolean(&mut self) -> Result<bool, Box<dyn Error>> {
        if self.raw_message.is_null() {
            return Err(Box::new(fmt::Error));
        }
        if self.cache.is_none() {
            if let Ok(b) = serde_json::from_value::<bool>(self.raw_message.clone()) {
                self.cache = Some(Value::Bool(b));
                return Ok(b);
            } else if let Ok(s) = serde_json::from_value::<String>(self.raw_message.clone()) {
                self.cache = Some(Value::String(s.clone()));
                return Ok(!s.is_empty());
            } else if let Ok(i) = serde_json::from_value::<i64>(self.raw_message.clone()) {
                self.cache = Some(Value::Number(i.into()));
                return Ok(i != 0);
            } else {
                return Err(Box::new(fmt::Error));
            }
        }
        match self.cache {
            Some(Value::Bool(b)) => Ok(b),
            Some(Value::String(ref s)) => Ok(!s.is_empty()),
            Some(Value::Number(ref n)) => Ok(n.as_i64().unwrap_or(0) != 0),
            _ => Err(Box::new(fmt::Error)),
        }
    }

    pub fn get_int_strict(&mut self) -> Result<i64, Box<dyn Error>> {
        if self.raw_message.is_null() {
            return Err(Box::new(fmt::Error));
        }
        if self.cache.is_none() {
            let i: i64 = serde_json::from_value(self.raw_message.clone())?;
            self.cache = Some(Value::Number(i.into()));
            return Ok(i);
        }
        if let Some(Value::Number(ref n)) = self.cache {
            return Ok(n.as_i64().unwrap());
        }
        Err(Box::new(fmt::Error))
    }

    pub fn get_int(&mut self) -> Result<i64, Box<dyn Error>> {
        if self.raw_message.is_null() {
            return Err(Box::new(fmt::Error));
        }
        if self.cache.is_none() {
            if let Ok(i) = serde_json::from_value::<i64>(self.raw_message.clone()) {
                self.cache = Some(Value::Number(i.into()));
                return Ok(i);
            } else if let Ok(s) = serde_json::from_value::<String>(self.raw_message.clone()) {
                self.cache = Some(Value::String(s.clone()));
                return Ok(s.parse::<i64>()?);
            } else if let Ok(b) = serde_json::from_value::<bool>(self.raw_message.clone()) {
                self.cache = Some(Value::Bool(b));
                return Ok(if b { 1 } else { 0 });
            } else {
                return Err(Box::new(fmt::Error));
            }
        }
        match self.cache {
            Some(Value::Number(ref n)) => Ok(n.as_i64().unwrap()),
            Some(Value::String(ref s)) => Ok(s.parse::<i64>()?),
            Some(Value::Bool(b)) => Ok(if b { 1 } else { 0 }),
            _ => Err(Box::new(fmt::Error)),
        }
    }

    pub fn get_big_int(&mut self) -> Result<BigDecimal, Box<dyn Error>> {
        if self.raw_message.is_null() {
            return Err(Box::new(fmt::Error));
        }
        if self.cache.is_none() {
            if let Ok(i) = serde_json::from_value::<i64>(self.raw_message.clone()) {
                self.cache = Some(Value::Number(i.into()));
                return Ok(BigDecimal::from(i));
            } else if let Ok(s) = serde_json::from_value::<String>(self.raw_message.clone()) {
                self.cache = Some(Value::String(s.clone()));
                return Ok(BigDecimal::from_str(&s)?);
            } else if let Ok(b) = serde_json::from_value::<bool>(self.raw_message.clone()) {
                self.cache = Some(Value::Bool(b));
                return Ok(BigDecimal::from(if b { 1 } else { 0 }));
            } else {
                return Err(Box::new(fmt::Error));
            }
        }
        match self.cache {
            Some(Value::Number(ref n)) => Ok(BigDecimal::from(n.as_i64().unwrap())),
            Some(Value::String(ref s)) => Ok(BigDecimal::from_str(s)?),
            Some(Value::Bool(b)) => Ok(BigDecimal::from(if b { 1 } else { 0 })),
            _ => Err(Box::new(fmt::Error)),
        }
    }

    pub fn get_array(&mut self) -> Result<Vec<Param>, Box<dyn Error>> {
        if self.raw_message.is_null() {
            return Err(Box::new(fmt::Error));
        }
        if self.cache.is_none() {
            let a: Vec<Param> = serde_json::from_value(self.raw_message.clone())?;
            self.cache = Some(Value::Array(a.iter().map(|p| p.raw_message.clone()).collect()));
            return Ok(a);
        }
        if let Some(Value::Array(ref a)) = self.cache {
            return Ok(a.iter().map(|v| Param { raw_message: v.clone(), cache: None }).collect());
        }
        Err(Box::new(fmt::Error))
    }

    pub fn get_uint256(&mut self) -> Result<util::Uint256, Box<dyn Error>> {
        let s = self.get_string()?;
        util::Uint256::from_str_radix(&s.trim_start_matches("0x"), 16).map_err(|e| Box::new(e) as Box<dyn Error>)
    }

    pub fn get_uint160_from_hex(&mut self) -> Result<util::Uint160, Box<dyn Error>> {
        let s = self.get_string()?;
        util::Uint160::from_str_radix(&s.trim_start_matches("0x"), 16).map_err(|e| Box::new(e) as Box<dyn Error>)
    }

    pub fn get_uint160_from_address(&mut self) -> Result<util::Uint160, Box<dyn Error>> {
        let s = self.get_string()?;
        address::string_to_uint160(&s).map_err(|e| Box::new(e) as Box<dyn Error>)
    }

    pub fn get_uint160_from_address_or_hex(&mut self) -> Result<util::Uint160, Box<dyn Error>> {
        self.get_uint160_from_hex().or_else(|_| self.get_uint160_from_address())
    }

    pub fn get_func_param(&mut self) -> Result<FuncParam, Box<dyn Error>> {
        if self.raw_message.is_null() {
            return Err(Box::new(fmt::Error));
        }
        let fp: FuncParam = serde_json::from_value(self.raw_message.clone())?;
        Ok(fp)
    }

    pub fn get_func_param_pair(&mut self) -> Result<FuncParamKV, Box<dyn Error>> {
        if self.raw_message.is_null() {
            return Err(Box::new(fmt::Error));
        }
        let fpp: FuncParamKV = serde_json::from_value(self.raw_message.clone())?;
        Ok(fpp)
    }

    pub fn get_bytes_hex(&mut self) -> Result<Vec<u8>, Box<dyn Error>> {
        let s = self.get_string()?;
        hex::decode(&s).map_err(|e| Box::new(e) as Box<dyn Error>)
    }

    pub fn get_bytes_base64(&mut self) -> Result<Vec<u8>, Box<dyn Error>> {
        let s = self.get_string()?;
        base64::decode(&s).map_err(|e| Box::new(e) as Box<dyn Error>)
    }

    pub fn get_signer_with_witness(&mut self) -> Result<neorpc::SignerWithWitness, Box<dyn Error>> {
        let c: neorpc::SignerWithWitness = serde_json::from_value(self.raw_message.clone())?;
        Ok(c)
    }

    pub fn get_signers_with_witnesses(&mut self) -> Result<(Vec<Signer>, Vec<Witness>), Box<dyn Error>> {
        let hashes = self.get_array()?;
        if hashes.len() > transaction::MAX_ATTRIBUTES {
            return Err(Box::new(fmt::Error));
        }
        let mut signers = Vec::with_capacity(hashes.len());
        let mut witnesses = Vec::with_capacity(hashes.len());
        for h in &hashes {
            if let Ok(u) = h.get_uint160_from_hex() {
                signers.push(Signer {
                    account: u,
                    scopes: transaction::CalledByEntry,
                });
            } else {
                for h in &hashes {
                    let signer_with_witness = h.get_signer_with_witness()?;
                    signers.push(signer_with_witness.signer);
                    witnesses.push(signer_with_witness.witness);
                }
                break;
            }
        }
        Ok((signers, witnesses))
    }

    pub fn is_null(&self) -> bool {
        self.raw_message.is_null()
    }

    pub fn get_uuid(&mut self) -> Result<Uuid, Box<dyn Error>> {
        let s = self.get_string()?;
        Uuid::parse_str(&s).map_err(|e| Box::new(e) as Box<dyn Error>)
    }
}
