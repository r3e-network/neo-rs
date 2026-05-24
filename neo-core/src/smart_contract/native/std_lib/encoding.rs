use super::StdLib;
use crate::cryptography::{Base58, Hex};
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use base64::{engine::general_purpose, Engine as _};

impl StdLib {
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
}
