extern crate aes;
extern crate block_modes;

use aes::Aes256;
use block_modes::{BlockMode, Cbc};
use block_modes::block_padding::Pkcs7;
use std::error::Error;

type Aes256Cbc = Cbc<Aes256, Pkcs7>;

// aes_encrypt encrypts the key with the given source.
fn aes_encrypt(src: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let cipher = Aes256Cbc::new_var(key, iv)?;
    let ciphertext = cipher.encrypt_vec(src);
    Ok(ciphertext)
}

// aes_decrypt decrypts the encrypted source with the given key.
fn aes_decrypt(crypted: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let cipher = Aes256Cbc::new_var(key, iv)?;
    let decrypted_ciphertext = cipher.decrypt_vec(crypted)?;
    Ok(decrypted_ciphertext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_encrypt_decrypt() {
        let key = b"an example very very secret key.";
        let iv = b"unique nonce";
        let plaintext = b"Hello, world!";

        let encrypted = aes_encrypt(plaintext, key, iv).unwrap();
        let decrypted = aes_decrypt(&encrypted, key, iv).unwrap();

        assert_eq!(plaintext.to_vec(), decrypted);
    }
}
