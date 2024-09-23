use std::error::Error;
use std::fmt;
use std::sync::Arc;

use aes::Aes256;
use aes::cipher::{BlockEncrypt, KeyIvInit};
use base58::ToBase58;
use scrypt::{scrypt, Params as ScryptParams};
use unicode_normalization::UnicodeNormalization;

use crate::crypto::hash;
use crate::crypto::keys::PrivateKey;

// NEP-2 standard implementation for encrypting and decrypting private keys.

// NEP-2 specified parameters used for cryptography.
const N: u32 = 16384;
const R: u32 = 8;
const P: u32 = 8;
const KEY_LEN: usize = 64;
const NEP_FLAG: u8 = 0xe0;

const NEP_HEADER: [u8; 2] = [0x01, 0x42];

#[derive(Serialize, Deserialize)]
pub struct ScryptParams {
    n: u32,
    r: u32,
    p: u32,
}

impl ScryptParams {
    pub fn new() -> Self {
        ScryptParams {
            n: N,
            r: R,
            p: P,
        }
    }
}

// NEP2Encrypt encrypts the PrivateKey using the given passphrase
// under the NEP-2 standard.
pub fn nep2_encrypt(priv_key: &PrivateKey, passphrase: &str, params: ScryptParams) -> Result<String, Box<dyn Error>> {
    let address = priv_key.address();
    let addr_hash = hash::checksum(address.as_bytes());

    // Normalize the passphrase according to the NFC standard.
    let phrase_norm: Vec<u8> = passphrase.nfc().collect();
    let mut derived_key = vec![0u8; KEY_LEN];
    scrypt(&phrase_norm, &addr_hash, &ScryptParams::new(), &mut derived_key)?;

    let derived_key1 = &derived_key[..32];
    let derived_key2 = &derived_key[32..];

    let priv_bytes = priv_key.to_bytes();
    let xr = xor(&priv_bytes, derived_key1);

    let encrypted = aes_encrypt(&xr, derived_key2)?;

    let mut buf = Vec::with_capacity(NEP_HEADER.len() + 1 + addr_hash.len() + encrypted.len());
    buf.extend_from_slice(&NEP_HEADER);
    buf.push(NEP_FLAG);
    buf.extend_from_slice(&addr_hash);
    buf.extend_from_slice(&encrypted);

    Ok(buf.to_base58())
}

// NEP2Decrypt decrypts an encrypted key using the given passphrase
// under the NEP-2 standard.
pub fn nep2_decrypt(key: &str, passphrase: &str, params: ScryptParams) -> Result<PrivateKey, Box<dyn Error>> {
    let b = base58::FromBase58::from_base58(key)?;
    validate_nep2_format(&b)?;

    let addr_hash = &b[3..7];

    // Normalize the passphrase according to the NFC standard.
    let phrase_norm: Vec<u8> = passphrase.nfc().collect();
    let mut derived_key = vec![0u8; KEY_LEN];
    scrypt(&phrase_norm, addr_hash, &ScryptParams::new(), &mut derived_key)?;

    let derived_key1 = &derived_key[..32];
    let derived_key2 = &derived_key[32..];
    let encrypted_bytes = &b[7..];

    let decrypted = aes_decrypt(encrypted_bytes, derived_key2)?;
    let priv_bytes = xor(&decrypted, derived_key1);

    // Rebuild the private key.
    let priv_key = PrivateKey::from_bytes(&priv_bytes)?;

    if !compare_address_hash(&priv_key, addr_hash) {
        return Err(Box::new(fmt::Error::new("password mismatch")));
    }

    Ok(priv_key)
}

fn compare_address_hash(priv_key: &PrivateKey, inhash: &[u8]) -> bool {
    let address = priv_key.address();
    let addr_hash = hash::checksum(address.as_bytes());
    addr_hash == inhash
}

fn validate_nep2_format(b: &[u8]) -> Result<(), Box<dyn Error>> {
    if b.len() != 39 {
        return Err(Box::new(fmt::Error::new(format!("invalid length: expecting 39 got {}", b.len()))));
    }
    if b[0] != 0x01 {
        return Err(Box::new(fmt::Error::new(format!("invalid byte sequence: expecting 0x01 got 0x{:02x}", b[0]))));
    }
    if b[1] != 0x42 {
        return Err(Box::new(fmt::Error::new(format!("invalid byte sequence: expecting 0x42 got 0x{:02x}", b[1]))));
    }
    if b[2] != 0xe0 {
        return Err(Box::new(fmt::Error::new(format!("invalid byte sequence: expecting 0xe0 got 0x{:02x}", b[2]))));
    }
    Ok(())
}

fn xor(a: &[u8], b: &[u8]) -> Vec<u8> {
    if a.len() != b.len() {
        panic!("cannot XOR non equal length arrays");
    }
    a.iter().zip(b.iter()).map(|(x, y)| x ^ y).collect()
}

fn aes_encrypt(data: &[u8], key: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let cipher = Aes256::new_from_slices(key, &[0u8; 16])?;
    let mut buffer = data.to_vec();
    cipher.encrypt(&mut buffer);
    Ok(buffer)
}

fn aes_decrypt(data: &[u8], key: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let cipher = Aes256::new_from_slices(key, &[0u8; 16])?;
    let mut buffer = data.to_vec();
    cipher.decrypt(&mut buffer);
    Ok(buffer)
}
