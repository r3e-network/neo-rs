use super::*;
use crate::cryptography::{Base58, Hex};
use base64::{Engine as _, engine::general_purpose};
use num_bigint::{BigInt, Sign};
use num_traits::{Num, ToPrimitive, Zero};

impl StdLib {
    /// Converts a string to an integer.
    pub(super) fn atoi(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "atoi requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "atoi")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        let base = self.parse_optional_base(args, 1, "atoi")?;
        let value = match base {
            10 => string_data
                .parse::<BigInt>()
                .map_err(|_| Error::native_contract("Invalid number format"))?,
            16 => self.parse_hex_twos_complement(&string_data)?,
            _ => return Err(Error::native_contract(format!("Invalid base: {}", base))),
        };

        Ok(value.to_signed_bytes_le())
    }

    /// Converts an integer to a string.
    pub(super) fn itoa(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "itoa requires integer argument".to_string(),
            ));
        }

        let value = BigInt::from_signed_bytes_le(&args[0]);
        let base = self.parse_optional_base(args, 1, "itoa")?;
        let encoded = match base {
            10 => value.to_string(),
            16 => self.format_hex_twos_complement(&value),
            _ => return Err(Error::native_contract(format!("Invalid base: {}", base))),
        };

        Ok(encoded.into_bytes())
    }

    /// Encodes data to base64.
    pub(super) fn base64_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base64Encode requires data argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base64Encode")?;
        let encoded = general_purpose::STANDARD.encode(&args[0]);
        Ok(encoded.into_bytes())
    }

    /// Decodes data from base64.
    pub(super) fn base64_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base64Decode requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base64Decode")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;

        let normalized: String = string_data.chars().filter(|c| !c.is_whitespace()).collect();
        let decoded = general_purpose::STANDARD
            .decode(normalized.as_bytes())
            .map_err(|_| Error::native_contract("Invalid base64 data"))?;

        Ok(decoded)
    }

    /// Encodes a string to base64url.
    pub(super) fn base64_url_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base64UrlEncode requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base64UrlEncode")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        let encoded = general_purpose::URL_SAFE_NO_PAD.encode(string_data.as_bytes());
        Ok(encoded.into_bytes())
    }

    /// Decodes a string from base64url.
    pub(super) fn base64_url_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base64UrlDecode requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base64UrlDecode")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        let normalized: String = string_data.chars().filter(|c| !c.is_whitespace()).collect();
        let decoded = general_purpose::URL_SAFE_NO_PAD
            .decode(normalized.as_bytes())
            .map_err(|_| Error::native_contract("Invalid base64url data"))?;
        let decoded_string = String::from_utf8(decoded)
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        Ok(decoded_string.into_bytes())
    }

    /// Encodes bytes to base58.
    pub(super) fn base58_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base58Encode requires data argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base58Encode")?;
        Ok(Base58::encode(&args[0]).into_bytes())
    }

    /// Decodes a base58 string to bytes.
    pub(super) fn base58_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base58Decode requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base58Decode")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        Base58::decode(&string_data)
            .map_err(|e| Error::native_contract(format!("Invalid base58 data: {e}")))
    }

    /// Encodes bytes to base58check.
    pub(super) fn base58_check_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base58CheckEncode requires data argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base58CheckEncode")?;
        Ok(Base58::encode_check(&args[0]).into_bytes())
    }

    /// Decodes a base58check string to bytes.
    pub(super) fn base58_check_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base58CheckDecode requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base58CheckDecode")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        Base58::decode_check(&string_data)
            .map_err(|e| Error::native_contract(format!("Invalid base58check data: {e}")))
    }

    /// Encodes bytes to hex.
    pub(super) fn hex_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "hexEncode requires data argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "hexEncode")?;
        Ok(Hex::encode(&args[0]).into_bytes())
    }

    /// Decodes hex string to bytes.
    pub(super) fn hex_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "hexDecode requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "hexDecode")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        Hex::decode(&string_data)
            .map_err(|e| Error::native_contract(format!("Invalid hex data: {e}")))
    }

    pub(super) fn ensure_max_input_len(&self, data: &[u8], method: &str) -> Result<()> {
        if data.len() > Self::MAX_INPUT_LENGTH {
            return Err(Error::native_contract(format!(
                "{} input exceeds max length {}",
                method,
                Self::MAX_INPUT_LENGTH
            )));
        }
        Ok(())
    }

    pub(super) fn parse_optional_base(
        &self,
        args: &[Vec<u8>],
        index: usize,
        method: &str,
    ) -> Result<i32> {
        if args.len() <= index {
            return Ok(10);
        }
        let base = BigInt::from_signed_bytes_le(&args[index]);
        base.to_i32()
            .ok_or_else(|| Error::native_contract(format!("Invalid base argument for {}", method)))
    }

    fn parse_hex_twos_complement(&self, input: &str) -> Result<BigInt> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(Error::native_contract(
                "Invalid hex number format".to_string(),
            ));
        }
        if trimmed.starts_with('+') || trimmed.starts_with('-') {
            return Err(Error::native_contract(
                "Invalid hex number format".to_string(),
            ));
        }

        let normalized = trimmed.to_ascii_lowercase();
        let unsigned = BigInt::from_str_radix(&normalized, 16)
            .map_err(|_| Error::native_contract("Invalid hex number format"))?;
        let bits = trimmed
            .len()
            .checked_mul(4)
            .ok_or_else(|| Error::native_contract("Hex value too large"))?;
        if bits == 0 {
            return Ok(BigInt::from(0));
        }

        let sign_bit = BigInt::from(1) << (bits - 1);
        if (&unsigned & &sign_bit) != BigInt::from(0) {
            let modulus = BigInt::from(1) << bits;
            Ok(unsigned - modulus)
        } else {
            Ok(unsigned)
        }
    }

    fn format_hex_twos_complement(&self, value: &BigInt) -> String {
        if value.is_zero() {
            return "0".to_string();
        }
        if value.sign() != Sign::Minus {
            let hex = value.to_str_radix(16);
            let requires_sign_padding =
                hex.len() % 2 == 0 && matches!(hex.as_bytes().first(), Some(b'8'..=b'f'));
            return if requires_sign_padding {
                format!("0{hex}")
            } else {
                hex
            };
        }

        let abs_value = (-value).to_biguint().unwrap_or_default();
        let bit_len = abs_value.to_str_radix(2).len();
        let is_power_of_two = !abs_value.is_zero() && (&abs_value & (&abs_value - 1u32)).is_zero();
        let bits_required = if is_power_of_two {
            bit_len
        } else {
            bit_len + 1
        };
        let nibbles = bits_required.div_ceil(4);
        let bits = nibbles * 4;
        let modulus = BigInt::from(1) << bits;
        let unsigned = modulus + value;
        let mut hex = unsigned.to_str_radix(16);
        if hex.len() < nibbles {
            let padding = "0".repeat(nibbles - hex.len());
            hex = format!("{}{}", padding, hex);
        }
        hex
    }
}
