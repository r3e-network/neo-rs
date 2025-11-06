use alloc::{string::String, vec::Vec};

use neo_base::{
    encoding::{FromBase58Check, FromBase58CheckError, ToBase58Check},
    hash::double_sha256,
    AddressVersion,
};
use zeroize::Zeroizing;

use crate::{
    aes::{Aes256EcbCipher, AES256_KEY_SIZE},
    ecc256::{Keypair, PrivateKey},
    scrypt::{DeriveScryptKey, ScryptDeriveError, ScryptParams},
};

#[derive(Debug, Clone, thiserror::Error)]
pub enum Nep2Error {
    #[error("nep2: invalid format")]
    InvalidFormat,

    #[error("nep2: invalid address hash")]
    InvalidAddressHash,

    #[error("nep2: invalid private key")]
    InvalidPrivateKey,

    #[error("nep2: base58 decode failed")]
    Base58,

    #[error("nep2: scrypt {0}")]
    Scrypt(#[from] ScryptDeriveError),

    #[error("nep2: aes-ecb {0}")]
    Aes(#[from] crate::aes::AesEcbError),
}

impl From<FromBase58CheckError> for Nep2Error {
    fn from(_value: FromBase58CheckError) -> Self {
        Nep2Error::Base58
    }
}

/// Encrypt a 32-byte private key into a NEP-2 Base58 string.
pub fn encrypt_nep2(
    private: &PrivateKey,
    passphrase: impl AsRef<[u8]>,
    version: AddressVersion,
    params: ScryptParams,
) -> Result<String, Nep2Error> {
    const HEADER: [u8; 3] = [0x01, 0x42, 0xE0];

    let keypair =
        Keypair::from_private(private.clone()).map_err(|_| Nep2Error::InvalidPrivateKey)?;
    let script_hash = keypair.public_key.script_hash();
    let address = script_hash.to_address(version);

    let address_hash_full = double_sha256(address.as_bytes());
    let mut address_hash = [0u8; 4];
    address_hash.copy_from_slice(&address_hash_full[..4]);

    let mut derived = passphrase.derive_scrypt_key::<64>(&address_hash, params)?;
    let (derived_half1, derived_half2) = derived.as_mut().split_at_mut(AES256_KEY_SIZE);

    let mut plain = Zeroizing::new(private.as_be_bytes().to_vec());
    for (byte, mask) in plain.iter_mut().zip(derived_half1.iter()) {
        *byte ^= *mask;
    }
    derived_half2.aes256_ecb_encrypt_aligned(plain.as_mut())?;

    let mut buffer = [0u8; 39];
    buffer[..3].copy_from_slice(&HEADER);
    buffer[3..7].copy_from_slice(&address_hash);
    buffer[7..].copy_from_slice(plain.as_ref());

    Ok(buffer.to_base58_check())
}

/// Decrypt a NEP-2 string back into the 32-byte private key.
pub fn decrypt_nep2(
    nep2: &str,
    passphrase: impl AsRef<[u8]>,
    version: AddressVersion,
    params: ScryptParams,
) -> Result<PrivateKey, Nep2Error> {
    let data = Vec::<u8>::from_base58_check(nep2)?;
    if data.len() != 39 || data[0] != 0x01 || data[1] != 0x42 || data[2] != 0xE0 {
        return Err(Nep2Error::InvalidFormat);
    }

    let mut address_hash = [0u8; 4];
    address_hash.copy_from_slice(&data[3..7]);
    let encrypted_key = &data[7..];

    let mut derived = passphrase.derive_scrypt_key::<64>(&address_hash, params)?;
    let (derived_half1, derived_half2) = derived.as_mut().split_at_mut(AES256_KEY_SIZE);

    let mut decrypted = Zeroizing::new(encrypted_key.to_vec());
    derived_half2.aes256_ecb_decrypt_aligned(decrypted.as_mut())?;
    for (byte, mask) in decrypted.iter_mut().zip(derived_half1.iter()) {
        *byte ^= *mask;
    }

    let mut private_bytes = [0u8; 32];
    private_bytes.copy_from_slice(decrypted.as_ref());
    let private =
        PrivateKey::from_slice(&private_bytes).map_err(|_| Nep2Error::InvalidPrivateKey)?;

    let keypair =
        Keypair::from_private(private.clone()).map_err(|_| Nep2Error::InvalidPrivateKey)?;
    let address = keypair.public_key.script_hash().to_address(version);
    let checksum = double_sha256(address.as_bytes());

    if checksum[..4] != address_hash {
        return Err(Nep2Error::InvalidAddressHash);
    }

    Ok(private)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_base::AddressVersion;

    const TEST_PARAMS: ScryptParams = ScryptParams { n: 2, r: 1, p: 1 };
    const TEST_VECTOR: &str = "6PYRzCDe46gkaR1E9AX3GyhLgQehypFvLG2KknbYjeNHQ3MZR2iqg8mcN3";

    #[test]
    fn nep2_encrypt_matches_vector() {
        let private = PrivateKey::new([1u8; 32]);
        let nep2 = encrypt_nep2(&private, "Satoshi", AddressVersion::MAINNET, TEST_PARAMS).unwrap();
        assert_eq!(nep2, TEST_VECTOR);
    }

    #[test]
    fn nep2_decrypt_matches_vector() {
        let private =
            decrypt_nep2(TEST_VECTOR, "Satoshi", AddressVersion::MAINNET, TEST_PARAMS).unwrap();
        assert_eq!(private.as_be_bytes(), [1u8; 32]);
    }

    #[test]
    fn nep2_roundtrip() {
        let private = PrivateKey::new([1u8; 32]);
        let nep2 = encrypt_nep2(&private, "Satoshi", AddressVersion::MAINNET, TEST_PARAMS).unwrap();
        let restored =
            decrypt_nep2(&nep2, "Satoshi", AddressVersion::MAINNET, TEST_PARAMS).unwrap();
        assert_eq!(restored.as_be_bytes(), private.as_be_bytes());
    }

    #[test]
    fn nep2_invalid_password() {
        let result = decrypt_nep2(TEST_VECTOR, "wrong", AddressVersion::MAINNET, TEST_PARAMS);
        assert!(matches!(result, Err(Nep2Error::InvalidAddressHash)));
    }
}
