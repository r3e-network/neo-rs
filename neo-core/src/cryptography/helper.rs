use sha2::{Sha256, Digest};
use ripemd::Ripemd160;
use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::{Aead, NewAead};
use std::convert::TryInto;

pub struct Helper;

impl Helper {
    pub fn ripemd160(value: &[u8]) -> Vec<u8> {
        let mut hasher = Ripemd160::new();
        hasher.update(value);
        hasher.finalize().to_vec()
    }

    pub fn murmur32(value: &[u8], seed: u32) -> u32 {
        use murmurhash32::murmurhash3;
        murmurhash3(value, seed)
    }

    pub fn murmur128(value: &[u8], seed: u32) -> Vec<u8> {
        use murmurhash64::murmur_hash64a;
        let hash = murmur_hash64a(value, seed as u64);
        hash.to_be_bytes().to_vec()
    }

    pub fn sha256(value: &[u8]) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(value);
        hasher.finalize().to_vec()
    }

    pub fn keccak256(value: &[u8]) -> Vec<u8> {
        use sha3::{Keccak256, Digest};
        let mut hasher = Keccak256::new();
        hasher.update(value);
        hasher.finalize().to_vec()
    }

    pub fn aes256_encrypt(plain_data: &[u8], key: &[u8], nonce: &[u8], associated_data: Option<&[u8]>) -> Vec<u8> {
        if nonce.len() != 12 {
            panic!("Nonce must be 12 bytes long");
        }

        let key = Key::from_slice(key);
        let nonce = Nonce::from_slice(nonce);
        let cipher = Aes256Gcm::new(key);

        let ciphertext = if let Some(aad) = associated_data {
            cipher.encrypt(nonce, aad.chain(plain_data.iter())).unwrap()
        } else {
            cipher.encrypt(nonce, plain_data).unwrap()
        };

        [nonce.as_slice(), &ciphertext].concat()
    }

    pub fn aes256_decrypt(encrypted_data: &[u8], key: &[u8], associated_data: Option<&[u8]>) -> Vec<u8> {
        let nonce = Nonce::from_slice(&encrypted_data[..12]);
        let ciphertext = &encrypted_data[12..];

        let key = Key::from_slice(key);
        let cipher = Aes256Gcm::new(key);

        if let Some(aad) = associated_data {
            cipher.decrypt(nonce, aad.chain(ciphertext.iter())).unwrap()
        } else {
            cipher.decrypt(nonce, ciphertext).unwrap()
        }
    }

    #[inline(always)]
    pub fn rotate_left_u32(value: u32, offset: i32) -> u32 {
        value.rotate_left(offset as u32)
    }

    #[inline(always)]
    pub fn rotate_left_u64(value: u64, offset: i32) -> u64 {
        value.rotate_left(offset as u32)
    }
}
