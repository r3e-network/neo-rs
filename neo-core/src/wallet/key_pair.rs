use std::fmt;
use aes_gcm::aes::Aes256;
use sha2::{Digest, Sha256};
use neo_base::encoding::base58;
use crate::contract::Contract;
use crate::cryptography::ECPoint;
use neo_type::H160;

/// Represents a private/public key pair in wallets.
#[derive(Clone)]
pub struct KeyPair {
    /// The private key.
    pub private_key: [u8; 32],

    /// The public key.
    pub public_key: ECPoint,
}

impl KeyPair {
    /// Initializes a new instance of the KeyPair struct.
    ///
    /// # Arguments
    ///
    /// * `private_key` - The private key in the KeyPair.
    ///
    /// # Returns
    ///
    /// A Result containing the new KeyPair instance or an error.
    pub fn new(private_key: &[u8]) -> Result<Self, String> {
        if private_key.len() != 32 && private_key.len() != 96 && private_key.len() != 104 {
            return Err("Invalid private key length".into());
        }
        
        let private_key = private_key[private_key.len() - 32..].try_into().unwrap();
        let public_key = if private_key.len() == 32 {
            ECPoint::from_private_key(&private_key)
        } else {
            ECPoint::from_bytes(&private_key, ECCurve::Secp256r1)?
        };

        Ok(Self { private_key, public_key })
    }

    /// The hash of the public key.
    pub fn public_key_hash(&self) -> H160 {
        self.public_key.to_script_hash()
    }

    /// Exports the private key in WIF format.
    ///
    /// # Returns
    ///
    /// The private key in WIF format.
    pub fn export(&self) -> String {
        let mut data = [0u8; 34];
        data[0] = 0x80;
        data[1..33].copy_from_slice(&self.private_key);
        data[33] = 0x01;
        base58::encode_check(&data)
    }

    /// Exports the private key in NEP-2 format.
    ///
    /// # Arguments
    ///
    /// * `passphrase` - The passphrase of the private key.
    /// * `version` - The address version.
    /// * `n` - The N field of the ScryptParameters to be used.
    /// * `r` - The R field of the ScryptParameters to be used.
    /// * `p` - The P field of the ScryptParameters to be used.
    ///
    /// # Returns
    ///
    /// The private key in NEP-2 format.
    pub fn export_nep2(&self, passphrase: &str, version: u8, n: u32, r: u32, p: u32) -> Result<String, String> {
        let passphrase = passphrase.as_bytes();
        self.export_nep2_raw(passphrase, version, n, r, p)
    }

    /// Exports the private key in NEP-2 format.
    ///
    /// # Arguments
    ///
    /// * `passphrase` - The passphrase of the private key.
    /// * `version` - The address version.
    /// * `n` - The N field of the ScryptParameters to be used.
    /// * `r` - The R field of the ScryptParameters to be used.
    /// * `p` - The P field of the ScryptParameters to be used.
    ///
    /// # Returns
    ///
    /// The private key in NEP-2 format.
    pub fn export_nep2_raw(&self, passphrase: &[u8], version: u8, n: u32, r: u32, p: u32) -> Result<String, String> {
        let script_hash = Contract::create_signature_redeemscript(&self.public_key).to_script_hash();
        let address = script_hash.to_address(version);
        let address_hash = Sha256::digest(&Sha256::digest(address.as_bytes()))[..4].to_vec();

        let scrypt_params = ScryptParams::new(n, r, p).map_err(|e| e.to_string())?;
        let mut derived_key = vec![0u8; 64];
        scrypt(passphrase, &address_hash, &scrypt_params, &mut derived_key).map_err(|e| e.to_string())?;

        let derived_half1 = &derived_key[..32];
        let derived_half2 = &derived_key[32..];

        let mut xored = [0u8; 32];
        for i in 0..32 {
            xored[i] = self.private_key[i] ^ derived_half1[i];
        }

        let encrypted_key = encrypt(&xored, derived_half2)?;

        let mut buffer = [0u8; 39];
        buffer[0] = 0x01;
        buffer[1] = 0x42;
        buffer[2] = 0xe0;
        buffer[3..7].copy_from_slice(&address_hash);
        buffer[7..].copy_from_slice(&encrypted_key);

        Ok(base58::encode_check(&buffer))
    }
}

impl PartialEq for KeyPair {
    fn eq(&self, other: &Self) -> bool {
        self.public_key == other.public_key
    }
}

impl Eq for KeyPair {}

impl fmt::Display for KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.public_key)
    }
}

fn encrypt(data: &[u8], key: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = Ecb::<Aes256, NoPadding>::new_from_slices(key, Default::default())
        .map_err(|e| e.to_string())?;
    cipher.encrypt_vec(data).map_err(|e| e.to_string())
}
