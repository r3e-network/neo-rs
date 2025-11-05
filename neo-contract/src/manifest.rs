use alloc::{string::String, vec::Vec};
use core::convert::TryFrom;

use neo_base::{
    hash::Hash160,
    write_varint,
    DecodeError,
    NeoDecode,
    NeoEncode,
    NeoRead,
    NeoWrite,
};
use serde::{Deserialize, Serialize};

use crate::error::ContractError;

fn encode_vec<W, T>(writer: &mut W, items: &[T])
where
    W: NeoWrite,
    T: NeoEncode,
{
    write_varint(writer, items.len() as u64);
    for item in items {
        item.neo_encode(writer);
    }
}

fn decode_vec<R, T>(reader: &mut R) -> Result<Vec<T>, DecodeError>
where
    R: NeoRead,
    T: NeoDecode,
{
    let len = reader.read_varint()?;
    if len > usize::MAX as u64 {
        return Err(DecodeError::LengthOutOfRange {
            len,
            max: usize::MAX as u64,
        });
    }

    let capacity = usize::try_from(len).expect("length fits in usize");
    let mut items = Vec::with_capacity(capacity);
    for _ in 0..len {
        items.push(T::neo_decode(reader)?);
    }
    Ok(items)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractManifest {
    pub name: String,
    pub groups: Vec<Hash160>,
    pub methods: Vec<ContractMethod>,
    pub permissions: Vec<Permission>,
}

impl ContractManifest {
    pub fn find_method(&self, name: &str) -> Option<&ContractMethod> {
        self.methods.iter().find(|m| m.name == name)
    }

    pub fn allows(&self, permission: PermissionKind) -> bool {
        self.permissions
            .iter()
            .any(|p| p.kind == permission || p.kind == PermissionKind::Global)
    }

    pub fn ensure_allowed(&self, permission: PermissionKind) -> Result<(), ContractError> {
        if self.allows(permission) {
            Ok(())
        } else {
            Err(ContractError::PermissionDenied(permission))
        }
    }
}

impl NeoEncode for ContractManifest {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.name.neo_encode(writer);
        encode_vec(writer, &self.groups);
        encode_vec(writer, &self.methods);
        encode_vec(writer, &self.permissions);
    }
}

impl NeoDecode for ContractManifest {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let name = String::neo_decode(reader)?;
        let groups: Vec<Hash160> = decode_vec(reader)?;
        let methods: Vec<ContractMethod> = decode_vec(reader)?;
        let permissions: Vec<Permission> = decode_vec(reader)?;
        Ok(Self {
            name,
            groups,
            methods,
            permissions,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractMethod {
    pub name: String,
    pub parameters: Vec<ContractParameter>,
    pub return_type: ParameterKind,
    pub safe: bool,
}

impl ContractMethod {
    pub fn parameter_count(&self) -> usize {
        self.parameters.len()
    }
}

impl NeoEncode for ContractMethod {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.name.neo_encode(writer);
        encode_vec(writer, &self.parameters);
        self.return_type.neo_encode(writer);
        self.safe.neo_encode(writer);
    }
}

impl NeoDecode for ContractMethod {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let name = String::neo_decode(reader)?;
        let parameters: Vec<ContractParameter> = decode_vec(reader)?;
        let return_type = ParameterKind::neo_decode(reader)?;
        let safe = bool::neo_decode(reader)?;
        Ok(Self {
            name,
            parameters,
            return_type,
            safe,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractParameter {
    pub name: String,
    pub kind: ParameterKind,
}

impl NeoEncode for ContractParameter {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.name.neo_encode(writer);
        self.kind.neo_encode(writer);
    }
}

impl NeoDecode for ContractParameter {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let name = String::neo_decode(reader)?;
        let kind = ParameterKind::neo_decode(reader)?;
        Ok(Self { name, kind })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ParameterKind {
    Boolean = 0,
    Integer = 1,
    ByteArray = 2,
    String = 3,
}

impl NeoEncode for ParameterKind {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u8(*self as u8);
    }
}

impl NeoDecode for ParameterKind {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        match reader.read_u8()? {
            0 => Ok(ParameterKind::Boolean),
            1 => Ok(ParameterKind::Integer),
            2 => Ok(ParameterKind::ByteArray),
            3 => Ok(ParameterKind::String),
            _ => Err(DecodeError::InvalidValue("ParameterKind")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum PermissionKind {
    Global = 0,
    Call = 1,
    Notify = 2,
}

impl NeoEncode for PermissionKind {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u8(*self as u8);
    }
}

impl NeoDecode for PermissionKind {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        match reader.read_u8()? {
            0 => Ok(PermissionKind::Global),
            1 => Ok(PermissionKind::Call),
            2 => Ok(PermissionKind::Notify),
            _ => Err(DecodeError::InvalidValue("PermissionKind")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Permission {
    pub kind: PermissionKind,
    pub contract: Option<Hash160>,
}

impl NeoEncode for Permission {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.kind.neo_encode(writer);
        match self.contract {
            Some(ref hash) => {
                writer.write_u8(1);
                hash.neo_encode(writer);
            }
            None => writer.write_u8(0),
        }
    }
}

impl NeoDecode for Permission {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let kind = PermissionKind::neo_decode(reader)?;
        let has_contract = reader.read_u8()?;
        let contract = match has_contract {
            0 => None,
            1 => Some(Hash160::neo_decode(reader)?),
            _ => return Err(DecodeError::InvalidValue("Permission.contract")),
        };
        Ok(Self { kind, contract })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_base::SliceReader;

    #[test]
    fn manifest_permission_check() {
        let manifest = ContractManifest {
            name: "policy".into(),
            groups: vec![],
            methods: vec![ContractMethod {
                name: "balanceOf".into(),
                parameters: vec![ContractParameter {
                    name: "account".into(),
                    kind: ParameterKind::ByteArray,
                }],
                return_type: ParameterKind::Integer,
                safe: true,
            }],
            permissions: vec![Permission {
                kind: PermissionKind::Call,
                contract: None,
            }],
        };

        assert!(manifest.allows(PermissionKind::Call));
        assert!(!manifest.allows(PermissionKind::Notify));
        manifest
            .ensure_allowed(PermissionKind::Call)
            .expect("call permitted");
        assert!(manifest
            .ensure_allowed(PermissionKind::Notify)
            .is_err());
    }

    #[test]
    fn manifest_roundtrip() {
        let manifest = ContractManifest {
            name: "demo".into(),
            groups: vec![Hash160::new([1; 20])],
            methods: vec![],
            permissions: vec![Permission {
                kind: PermissionKind::Global,
                contract: None,
            }],
        };
        let mut buf = Vec::new();
        manifest.neo_encode(&mut buf);
        let mut reader = SliceReader::new(buf.as_slice());
        let decoded = ContractManifest::neo_decode(&mut reader).unwrap();
        assert_eq!(decoded.name, manifest.name);
        assert!(decoded.allows(PermissionKind::Global));
    }
}
