// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

#[allow(deprecated)]
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes256;

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

        for chunk in data.chunks_mut(AES_BLOCK_SIZE) {
            #[allow(deprecated)]
            {
                cipher.encrypt_block(GenericArray::from_mut_slice(chunk));
            }
        }
        Ok(())
    }

    fn aes256_ecb_decrypt_aligned(&self, data: &mut [u8]) -> Result<(), AesEcbError> {
        let cipher = Aes256::new_from_slice(self.as_ref()).expect("aes256 key length is 32-bytes");
        if data.len() % AES_BLOCK_SIZE != 0 {
            return Err(AesEcbError::InvalidDataLength);
        }

        for chunk in data.chunks_mut(AES_BLOCK_SIZE) {
            #[allow(deprecated)]
            {
                cipher.decrypt_block(GenericArray::from_mut_slice(chunk));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn aes_ecb_roundtrip() {
        let key = hex!(
            "603deb1015ca71be2b73aef0857d7781
             1f352c073b6108d72d9810a30914dff4"
        );

        let mut block = hex!(
            "6bc1bee22e409f96e93d7e117393172a
             ae2d8a571e03ac9c9eb76fac45af8e51"
        )
        .to_vec();

        key.as_slice()
            .aes256_ecb_encrypt_aligned(&mut block)
            .unwrap();
        key.as_slice()
            .aes256_ecb_decrypt_aligned(&mut block)
            .unwrap();
        assert_eq!(
            block,
            hex!(
                "6bc1bee22e409f96e93d7e117393172a
                 ae2d8a571e03ac9c9eb76fac45af8e51"
            )
        );
    }
}
