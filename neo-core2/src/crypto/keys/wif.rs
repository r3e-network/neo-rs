use std::fmt;

use crate::encoding::base58;

const WIF_VERSION: u8 = 0x80;

// WIF represents a wallet import format.
pub struct WIF {
    // Version of the wallet import format. Default to 0x80.
    version: u8,

    // Bool to determine if the WIF is compressed or not.
    compressed: bool,

    // A reference to the PrivateKey which this WIF is created from.
    private_key: PrivateKey,

    // A string representation of the WIF.
    s: String,
}

impl WIF {
    // WIFEncode encodes the given private key into a WIF string.
    pub fn encode(key: &[u8], version: u8, compressed: bool) -> Result<String, fmt::Error> {
        let version = if version == 0x00 { WIF_VERSION } else { version };
        if key.len() != 32 {
            return Err(fmt::Error);
        }

        let mut buf = Vec::with_capacity(1 + key.len() + 1);
        buf.push(version);
        buf.extend_from_slice(key);
        if compressed {
            buf.push(0x01);
        }

        Ok(base58::check_encode(&buf))
    }

    // WIFDecode decodes the given WIF string into a WIF struct.
    pub fn decode(wif: &str, version: u8) -> Result<WIF, fmt::Error> {
        let (b, err) = base58::check_decode(wif);
        if err.is_err() {
            return Err(fmt::Error);
        }
        let b = b.unwrap();
        clear(&b);

        let version = if version == 0x00 { WIF_VERSION } else { version };
        let mut w = WIF {
            version,
            s: wif.to_string(),
            compressed: false,
            private_key: PrivateKey::default(), // Placeholder, will be set later
        };

        match b.len() {
            33 => {} // OK, uncompressed public key.
            34 => {
                // OK, compressed public key.
                // Check the compression flag.
                if b[33] != 0x01 {
                    return Err(fmt::Error);
                }
                w.compressed = true;
            }
            _ => {
                return Err(fmt::Error);
            }
        }

        if b[0] != version {
            return Err(fmt::Error);
        }

        // Derive the PrivateKey.
        w.private_key = PrivateKey::from_bytes(&b[1..33])?;
        Ok(w)
    }
}

fn clear(b: &[u8]) {
    // Implement the clear function to zero out the byte slice
    for byte in b {
        *byte = 0;
    }
}

#[derive(Default)]
pub struct PrivateKey {
    // Define the PrivateKey struct and its methods
}

impl PrivateKey {
    pub fn from_bytes(bytes: &[u8]) -> Result<PrivateKey, fmt::Error> {
        // Implement the method to create a PrivateKey from bytes
        Ok(PrivateKey::default())
    }
}
