use super::*;
use neo_vm::stack_item::InteropInterface as VmInteropInterface;
use std::any::Any;

const BLS_INTEROP_G1_AFFINE: u8 = 0x01;
const BLS_INTEROP_G1_PROJECTIVE: u8 = 0x02;
const BLS_INTEROP_G2_AFFINE: u8 = 0x03;
const BLS_INTEROP_G2_PROJECTIVE: u8 = 0x04;
const BLS_INTEROP_GT: u8 = 0x05;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bls12381Group {
    G1,
    G2,
    Gt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bls12381Kind {
    G1Affine,
    G1Projective,
    G2Affine,
    G2Projective,
    Gt,
}

impl Bls12381Kind {
    fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            BLS_INTEROP_G1_AFFINE => Some(Self::G1Affine),
            BLS_INTEROP_G1_PROJECTIVE => Some(Self::G1Projective),
            BLS_INTEROP_G2_AFFINE => Some(Self::G2Affine),
            BLS_INTEROP_G2_PROJECTIVE => Some(Self::G2Projective),
            BLS_INTEROP_GT => Some(Self::Gt),
            _ => None,
        }
    }

    fn tag(self) -> u8 {
        match self {
            Self::G1Affine => BLS_INTEROP_G1_AFFINE,
            Self::G1Projective => BLS_INTEROP_G1_PROJECTIVE,
            Self::G2Affine => BLS_INTEROP_G2_AFFINE,
            Self::G2Projective => BLS_INTEROP_G2_PROJECTIVE,
            Self::Gt => BLS_INTEROP_GT,
        }
    }

    pub(crate) fn group(self) -> Bls12381Group {
        match self {
            Self::G1Affine | Self::G1Projective => Bls12381Group::G1,
            Self::G2Affine | Self::G2Projective => Bls12381Group::G2,
            Self::Gt => Bls12381Group::Gt,
        }
    }

    fn expected_len(self) -> usize {
        match self {
            Self::G1Affine | Self::G1Projective => 48,
            Self::G2Affine | Self::G2Projective => 96,
            Self::Gt => 576,
        }
    }

    fn interface_type(self) -> &'static str {
        match self {
            Self::G1Affine => "Bls12381G1Affine",
            Self::G1Projective => "Bls12381G1Projective",
            Self::G2Affine => "Bls12381G2Affine",
            Self::G2Projective => "Bls12381G2Projective",
            Self::Gt => "Bls12381Gt",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Bls12381Interop {
    kind: Bls12381Kind,
    bytes: Vec<u8>,
}

impl Bls12381Interop {
    pub(crate) fn new(kind: Bls12381Kind, bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() != kind.expected_len() {
            return Err(Error::native_contract(
                "Invalid BLS12-381 point size".to_string(),
            ));
        }
        Ok(Self { kind, bytes })
    }

    pub(crate) fn from_encoded_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 2 {
            return Err(Error::native_contract(
                "Invalid BLS12-381 interop payload".to_string(),
            ));
        }
        let kind = Bls12381Kind::from_tag(data[0])
            .ok_or_else(|| Error::native_contract("Invalid BLS12-381 interop payload"))?;
        let bytes = data[1..].to_vec();
        Self::new(kind, bytes)
    }

    pub(crate) fn to_encoded_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes.len() + 1);
        out.push(self.kind.tag());
        out.extend_from_slice(&self.bytes);
        out
    }

    pub(crate) fn kind(&self) -> Bls12381Kind {
        self.kind
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl VmInteropInterface for Bls12381Interop {
    fn interface_type(&self) -> &str {
        self.kind.interface_type()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
