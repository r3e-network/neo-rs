// Copyright @ 2025 - Present, R3E Network
// All Rights Reserved

use neo_crypto::ecc256::{self, KEY_SIZE};

// 40 bytes = 1-byte CHECK_SIG_PUSH_DATA1 + 1-byte length + 33-bytes key + 1-byte OpCode + 4-bytes suffix
pub const CHECK_SIGN_SIZE: usize = 1 + 1 + (KEY_SIZE + 1) + 1 + 4;

pub struct CheckSign([u8; CHECK_SIGN_SIZE]);

impl CheckSign {
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn from_compressed_public_key(compressed_public_key: &[u8; KEY_SIZE + 1]) -> Self {
        let mut buf = [0u8; CHECK_SIGN_SIZE];

        const SIZE: usize = KEY_SIZE + 1; // PUBLIC_COMPRESSED_SIZE;
        buf[0] = 0x0C; // OpCode::PushData1
        buf[1] = SIZE as u8;

        buf[2..2 + SIZE].copy_from_slice(compressed_public_key.as_slice());

        buf[2 + SIZE] = 0x41; // OpCode::Syscall
        buf[3 + SIZE..].copy_from_slice(&[0x56u8, 0xe7, 0xb3, 0x27]);

        Self(buf)
    }
}

impl AsRef<[u8; CHECK_SIGN_SIZE]> for CheckSign {
    #[inline]
    fn as_ref(&self) -> &[u8; CHECK_SIGN_SIZE] {
        &self.0
    }
}

pub trait ToCheckSign {
    fn to_check_sign(&self) -> CheckSign;
}

impl ToCheckSign for ecc256::PublicKey {
    #[inline]
    fn to_check_sign(&self) -> CheckSign {
        CheckSign::from_compressed_public_key(&self.to_compressed())
    }
}
