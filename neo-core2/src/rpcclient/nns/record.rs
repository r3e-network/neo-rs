use std::error::Error;
use std::fmt;
use std::str;
use std::convert::TryFrom;
use num_bigint::BigInt;
use crate::smartcontract;
use crate::vm::stackitem::{Item, StackItem};

#[derive(Debug)]
pub struct RecordState {
    name: String,
    record_type: RecordType,
    data: String,
}

#[derive(Debug, Clone, Copy)]
pub enum RecordType {
    A = 1,
    CNAME = 5,
    TXT = 16,
    AAAA = 28,
}

impl smartcontract::Convertible for RecordType {
    fn to_sc_parameter(&self) -> Result<smartcontract::Parameter, Box<dyn Error>> {
        Ok(smartcontract::Parameter {
            param_type: smartcontract::ParameterType::Integer,
            value: BigInt::from(*self as u8),
        })
    }
}

impl RecordState {
    pub fn from_stack_item(itm: &Item) -> Result<Self, Box<dyn Error>> {
        let rs = itm.value().as_array().ok_or("not a struct")?;
        if rs.len() != 3 {
            return Err("wrong number of elements".into());
        }
        let name = rs[0].try_bytes().map_err(|e| format!("bad name: {}", e))?;
        let typ = rs[1].try_integer().map_err(|e| format!("bad type: {}", e))?;
        let data = rs[2].try_bytes().map_err(|e| format!("bad data: {}", e))?;
        let u64_typ = typ.to_u64().ok_or("bad type")?;
        if u64_typ > 255 {
            return Err("bad type".into());
        }
        Ok(RecordState {
            name: String::from_utf8(name)?,
            record_type: RecordType::try_from(u64_typ as u8)?,
            data: String::from_utf8(data)?,
        })
    }
}

impl TryFrom<u8> for RecordType {
    type Error = Box<dyn Error>;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(RecordType::A),
            5 => Ok(RecordType::CNAME),
            16 => Ok(RecordType::TXT),
            28 => Ok(RecordType::AAAA),
            _ => Err("invalid record type".into()),
        }
    }
}

impl fmt::Display for RecordType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
