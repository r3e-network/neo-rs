// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use bytes::BytesMut;
use neo_base::encoding::bin::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Bytes, CheckSign, ToCheckSign};

// The maximum length of invocation-script.
// It should fit 11/21 multi-signature for the committee.
pub const MAX_INVOCATION_SCRIPT: usize = 1024;

// The maximum length of verification-script.
pub const MAX_VERIFICATION_SCRIPT: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptType {
    Plain,
    Invocation,
    Verification,
}

// pub struct HashedScript(ScriptHash, Script);

#[derive(Debug, Clone, Default, Eq, PartialEq, BinDecode, BinEncode)]
pub struct Script {
    script: Bytes,
}

impl Script {
    pub fn len(&self) -> usize {
        self.script.len()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.script.as_bytes()
    }
}

impl From<&[u8]> for Script {
    fn from(value: &[u8]) -> Self {
        Self { script: value.to_vec().into() }
    }
}

impl From<Vec<u8>> for Script {
    fn from(value: Vec<u8>) -> Self {
        Self { script: value.into() }
    }
}

impl From<BytesMut> for Script {
    fn from(value: BytesMut) -> Self {
        Self { script: Bytes(value.into()) }
    }
}

impl AsRef<[u8]> for Script {
    fn as_ref(&self) -> &[u8] {
        self.script.as_ref()
    }
}

impl Serialize for Script {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.script.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Script {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self { script: Bytes::deserialize(deserializer)? })
    }
}

pub trait ToVerificationScript {
    fn to_verification_script(&self) -> CheckSign;
}

impl<T: ToCheckSign> ToVerificationScript for T {
    fn to_verification_script(&self) -> CheckSign {
        self.to_check_sign()
    }
}
