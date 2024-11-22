use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// A helper struct for base-58 encoder.
pub struct Base58;

impl Base58 {
    /// Represents the alphabet of the base-58 encoder.
    pub const ALPHABET: &'static str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

    /// Converts the specified string, which encodes binary data as base-58 digits, to an equivalent byte array.
    /// The encoded string contains the checksum of the binary data.
    pub fn base58_check_decode(input: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if input.is_empty() {
            return Err("Input is empty".into());
        }
        let buffer = Self::decode(input)?;
        if buffer.len() < 4 {
            return Err("Invalid input length".into());
        }
        let checksum = Self::sha256(&Self::sha256(&buffer[..buffer.len() - 4]));
        if buffer[buffer.len() - 4..] != checksum[..4] {
            return Err("Checksum mismatch".into());
        }
        Ok(buffer[..buffer.len() - 4].to_vec())
    }

    /// Converts a byte array to its equivalent string representation that is encoded with base-58 digits.
    /// The encoded string contains the checksum of the binary data.
    pub fn base58_check_encode(data: &[u8]) -> String {
        let checksum = Self::sha256(&Self::sha256(data));
        let mut buffer = Vec::with_capacity(data.len() + 4);
        buffer.extend_from_slice(data);
        buffer.extend_from_slice(&checksum[..4]);
        Self::encode(&buffer)
    }

    /// Converts the specified string, which encodes binary data as base-58 digits, to an equivalent byte array.
    pub fn decode(input: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut bi = BigInt::from(0);
        for (i, c) in input.chars().enumerate() {
            let digit = Self::ALPHABET.find(c)
                .ok_or_else(|| format!("Invalid Base58 character '{}' at position {}", c, i))?;
            bi = bi * Self::ALPHABET.len() + digit;
        }

        let leading_zero_count = input.chars().take_while(|&c| c == Self::ALPHABET.chars().next().unwrap()).count();
        let mut bytes = bi.to_bytes_be().1;
        let mut result = vec![0; leading_zero_count];
        result.append(&mut bytes);
        Ok(result)
    }

    /// Converts a byte array to its equivalent string representation that is encoded with base-58 digits.
    pub fn encode(input: &[u8]) -> String {
        let mut value = BigInt::from_bytes_be(num_bigint::Sign::Plus, input);
        let mut result = String::new();

        while value > BigInt::from(0) {
            let (new_value, remainder) = value.div_rem(&BigInt::from(Self::ALPHABET.len()));
            result.insert(0, Self::ALPHABET.chars().nth(remainder.to_usize().unwrap()).unwrap());
            value = new_value;
        }

        for &byte in input.iter().take_while(|&&x| x == 0) {
            result.insert(0, Self::ALPHABET.chars().next().unwrap());
        }
        result
    }

    // Helper function to calculate SHA256
    fn sha256(data: &[u8]) -> Vec<u8> {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode() {
        assert_eq!(Base58::encode(&[0]), "1");
        assert_eq!(Base58::encode(&[0, 0]), "11");
        assert_eq!(Base58::encode(&[0, 0, 0]), "111");
        assert_eq!(Base58::encode(&[1]), "2");
        assert_eq!(Base58::encode(&[255]), "5Q");
    }

    #[test]
    fn test_decode() {
        assert_eq!(Base58::decode("1").unwrap(), vec![0]);
        assert_eq!(Base58::decode("11").unwrap(), vec![0, 0]);
        assert_eq!(Base58::decode("111").unwrap(), vec![0, 0, 0]);
        assert_eq!(Base58::decode("2").unwrap(), vec![1]);
        assert_eq!(Base58::decode("5Q").unwrap(), vec![255]);
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let test_data = vec![
            vec![],
            vec![0],
            vec![1, 2, 3],
            vec![255, 254, 253],
            vec![0, 0, 0, 1, 2, 3],
        ];

        for data in test_data {
            let encoded = Base58::encode(&data);
            let decoded = Base58::decode(&encoded).unwrap();
            assert_eq!(data, decoded, "Roundtrip failed for {:?}", data);
        }
    }

    #[test]
    fn test_base58_check_encode_decode_roundtrip() {
        let test_data = vec![
            vec![],
            vec![0],
            vec![1, 2, 3],
            vec![255, 254, 253],
            vec![0, 0, 0, 1, 2, 3],
        ];

        for data in test_data {
            let encoded = Base58::base58_check_encode(&data);
            let decoded = Base58::base58_check_decode(&encoded).unwrap();
            assert_eq!(data, decoded, "Checksum roundtrip failed for {:?}", data);
        }
    }

    #[test]
    fn test_base58_check_decode_invalid_input() {
        assert!(Base58::base58_check_decode("").is_err());
        assert!(Base58::base58_check_decode("1").is_err());
        assert!(Base58::base58_check_decode("11").is_err());
        assert!(Base58::base58_check_decode("111").is_err());
        assert!(Base58::base58_check_decode("1111").is_err());
        assert!(Base58::base58_check_decode("11111").is_err());
    }

    #[test]
    fn test_decode_invalid_character() {
        assert!(Base58::decode("0").is_err());
        assert!(Base58::decode("O").is_err());
        assert!(Base58::decode("I").is_err());
        assert!(Base58::decode("l").is_err());
    }
}
