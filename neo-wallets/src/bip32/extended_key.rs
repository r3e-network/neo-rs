//! BIP-32 extended key derivation.

// Zeroize derive macro generates code that triggers false-positive unused_assignments
#![allow(unused_assignments)]

use super::key_path::KeyPath;
use neo_crypto::{Bip32Crypto, CryptoError, ECC, ECCurve, ECPoint};
use neo_error::{CoreError, CoreResult};
use zeroize::Zeroize;

/// BIP-32 extended private key with public key and chain code.
#[derive(Clone, Zeroize)]
#[zeroize(drop)]
pub struct ExtendedKey {
    /// Private key bytes — zeroized on drop.
    private_key: [u8; 32],
    /// Corresponding public key (not sensitive, skipped by zeroize).
    #[zeroize(skip)]
    public_key: ECPoint,
    /// BIP-32 chain code — zeroized on drop.
    chain_code: [u8; 32],
}

impl std::fmt::Debug for ExtendedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtendedKey")
            .field("private_key", &"[REDACTED]")
            .field("public_key", &self.public_key)
            .field("chain_code", &"[REDACTED]")
            .finish()
    }
}

impl ExtendedKey {
    /// Returns a reference to the private key bytes.
    pub fn private_key(&self) -> &[u8; 32] {
        &self.private_key
    }

    /// Returns a reference to the public key.
    pub fn public_key(&self) -> &ECPoint {
        &self.public_key
    }

    /// Returns a reference to the chain code bytes.
    pub fn chain_code(&self) -> &[u8; 32] {
        &self.chain_code
    }

    /// Create the master extended key from a seed.
    pub fn create(seed: &[u8], curve: Option<ECCurve>) -> CoreResult<Self> {
        let curve = curve.unwrap_or(ECCurve::Secp256r1);
        if matches!(curve, ECCurve::Ed25519) {
            return Err(CoreError::other("Ed25519 is not supported for BIP32"));
        }
        let i = Bip32Crypto::hmac_sha512(b"Bitcoin seed", seed).map_err(bip32_error_message)?;
        let mut private_key = [0u8; 32];
        let mut chain_code = [0u8; 32];
        private_key.copy_from_slice(&i[..32]);
        chain_code.copy_from_slice(&i[32..]);

        let public_key = ECC::generate_public_key(&private_key, curve)
            .map_err(|e| CoreError::other(e.to_string()))?;

        Ok(Self {
            private_key,
            public_key,
            chain_code,
        })
    }

    /// Create an extended key and derive it along a BIP-32 path.
    pub fn create_with_path(seed: &[u8], path: &str, curve: Option<ECCurve>) -> CoreResult<Self> {
        let key_path = KeyPath::parse(path)?;
        let mut ext_key = Self::create(seed, curve)?;
        for index in key_path.indices() {
            ext_key = ext_key.derive(*index)?;
        }
        Ok(ext_key)
    }

    /// Derive a child extended key by index.
    pub fn derive(&self, index: u32) -> CoreResult<Self> {
        let mut data = [0u8; 37];
        if index >= 0x8000_0000 {
            data[0] = 0;
            data[1..33].copy_from_slice(&self.private_key);
        } else {
            let pub_bytes = self
                .public_key
                .encode_point(true)
                .map_err(|e| CoreError::other(e.to_string()))?;
            if pub_bytes.len() != 33 {
                return Err(CoreError::other("Invalid public key length"));
            }
            data[..33].copy_from_slice(&pub_bytes);
        }
        data[33..].copy_from_slice(&index.to_be_bytes());

        let i = Bip32Crypto::hmac_sha512(&self.chain_code, &data).map_err(bip32_error_message)?;
        let il: &[u8; 32] = i[..32]
            .try_into()
            .expect("HMAC-SHA512 left half is 32 bytes");
        let mut chain_code = [0u8; 32];
        chain_code.copy_from_slice(&i[32..]);

        let private_key =
            Bip32Crypto::add_private_keys_mod_order(il, &self.private_key, self.public_key.curve())
                .map_err(bip32_error_message)?;
        let public_key = ECC::generate_public_key(&private_key, self.public_key.curve())
            .map_err(|e| CoreError::other(e.to_string()))?;

        Ok(Self {
            private_key,
            public_key,
            chain_code,
        })
    }
}

fn bip32_error_message(error: CryptoError) -> CoreError {
    CoreError::other(match error {
        CryptoError::InvalidArgument { message } => message,
        error => error.to_string(),
    })
}
