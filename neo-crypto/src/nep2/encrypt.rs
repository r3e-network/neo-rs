use alloc::string::String;

use neo_base::{encoding::ToBase58Check, hash::double_sha256, AddressVersion};
use zeroize::Zeroizing;

use crate::{
    aes::{Aes256EcbCipher, AES256_KEY_SIZE},
    ecc256::{Keypair, PrivateKey},
    scrypt::{DeriveScryptKey, ScryptParams},
};

use super::Nep2Error;

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
