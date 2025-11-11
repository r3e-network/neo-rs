use alloc::vec::Vec;

use neo_base::{encoding::FromBase58Check, hash::double_sha256, AddressVersion};
use zeroize::Zeroizing;

use crate::{
    aes::{Aes256EcbCipher, AES256_KEY_SIZE},
    ecc256::{Keypair, PrivateKey},
    scrypt::{DeriveScryptKey, ScryptParams},
};

use super::Nep2Error;

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
