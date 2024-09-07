// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use aes::Aes256;
use aes::cipher::{KeyInit, BlockEncrypt, BlockDecrypt, generic_array::GenericArray};

use neo_base::errors;
use crate::key::SecretKey;


pub const AES256_KEY_SIZE: usize = 32;
pub const AES128_KEY_SIZE: usize = 16;

const AES_BLOCK_SIZE: usize = 16;


#[derive(Debug, Clone, errors::Error)]
pub enum EcbError {
    #[error("aes-ecb: invalid data length")]
    InvalidDataLength,
}

pub trait Aes256EcbCipher {
    fn aes256_ecb_encrypt_aligned(&self, buf: &mut [u8]) -> Result<(), EcbError>;

    fn aes256_ecb_decrypt_aligned(&self, buf: &mut [u8]) -> Result<(), EcbError>;
}


impl Aes256EcbCipher for SecretKey<AES256_KEY_SIZE> {
    fn aes256_ecb_encrypt_aligned(&self, data: &mut [u8]) -> Result<(), EcbError> {
        let cipher = Aes256::new_from_slice(self.as_ref())
            .expect("aes256 key length is 32-bytes");

        if data.len() % AES_BLOCK_SIZE != 0 {
            return Err(EcbError::InvalidDataLength);
        }

        data.chunks_mut(AES_BLOCK_SIZE)
            .map(|chunk| GenericArray::from_mut_slice(chunk))
            .for_each(|block| cipher.encrypt_block(block));
        Ok(())
    }

    fn aes256_ecb_decrypt_aligned(&self, data: &mut [u8]) -> Result<(), EcbError> {
        let cipher = Aes256::new_from_slice(self.as_ref())
            .expect("aes256 key length is 32-bytes");

        if data.len() % AES_BLOCK_SIZE != 0 {
            return Err(EcbError::InvalidDataLength);
        }

        data.chunks_mut(AES_BLOCK_SIZE)
            .map(|chunk| GenericArray::from_mut_slice(chunk))
            .for_each(|block| cipher.decrypt_block(block));
        Ok(())
    }
}


#[cfg(test)]
mod test {
    use super::*;
    use crate::{key::SecretKey, rand::OsRand};

    #[test]
    fn test_aes256_ecb() {
        let key = SecretKey::<AES256_KEY_SIZE>::from_crypto_rand(&mut OsRand)
            .expect("gen key should be ok");

        let mut data = b"Hello world!....".clone();
        let _ = key.aes256_ecb_encrypt_aligned(data.as_mut_slice())
            .expect("encrypt should be ok");

        let mut buf = data.clone();
        let _ = key.aes256_ecb_decrypt_aligned(buf.as_mut_slice())
            .expect("decrypted should be ok");

        assert_eq!(b"Hello world!....", buf.as_slice());
    }
}