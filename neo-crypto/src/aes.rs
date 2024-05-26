// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use aes::Aes256;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};

pub const AES256_KEY_SIZE: usize = 32;

const AES_BLOCK_SIZE: usize = 16;

#[derive(Debug, Clone, thiserror::Error)]
pub enum AesEcbError {
    #[error("aes-ecb: invalid data length")]
    InvalidDataLength,
}

pub trait Aes256EcbCipher {
    fn aes256_ecb_encrypt_aligned(&self, buf: &mut [u8]) -> Result<(), AesEcbError>;

    fn aes256_ecb_decrypt_aligned(&self, buf: &mut [u8]) -> Result<(), AesEcbError>;
}

impl Aes256EcbCipher for [u8] {
    fn aes256_ecb_encrypt_aligned(&self, data: &mut [u8]) -> Result<(), AesEcbError> {
        let cipher = Aes256::new_from_slice(self.as_ref()).expect("aes256 key length is 32-bytes");
        if data.len() % AES_BLOCK_SIZE != 0 {
            return Err(AesEcbError::InvalidDataLength);
        }

        data.chunks_mut(AES_BLOCK_SIZE)
            .for_each(|block| cipher.encrypt_block(block.into()));
        Ok(())
    }

    fn aes256_ecb_decrypt_aligned(&self, data: &mut [u8]) -> Result<(), AesEcbError> {
        let cipher = Aes256::new_from_slice(self.as_ref()).expect("aes256 key length is 32-bytes");
        if data.len() % AES_BLOCK_SIZE != 0 {
            return Err(AesEcbError::InvalidDataLength);
        }

        data.chunks_mut(AES_BLOCK_SIZE)
            .for_each(|block| cipher.decrypt_block(block.into()));
        Ok(())
    }
}
