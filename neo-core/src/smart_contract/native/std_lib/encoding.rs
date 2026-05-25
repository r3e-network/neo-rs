use super::StdLib;
use crate::cryptography::{Base58, Base64, Hex};
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;

impl StdLib {
    pub(super) fn base64_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let data = self.validate_single_arg(args, "base64Encode")?;
        Ok(Base64::encode(data).into_bytes())
    }

    pub(super) fn base64_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let string_data = self.validate_string_arg(args, "base64Decode")?;
        Base64::decode_lenient(&string_data)
            .map_err(|_| Error::native_contract("Invalid base64 data"))
    }

    pub(super) fn base64_url_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let string_data = self.validate_string_arg(args, "base64UrlEncode")?;
        Ok(Base64::url_encode_no_pad(string_data.as_bytes()).into_bytes())
    }

    pub(super) fn base64_url_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let string_data = self.validate_string_arg(args, "base64UrlDecode")?;
        let decoded = Base64::url_decode_no_pad_lenient(&string_data)
            .map_err(|_| Error::native_contract("Invalid base64url data"))?;
        let decoded_string = String::from_utf8(decoded)
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        Ok(decoded_string.into_bytes())
    }

    pub(super) fn base58_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let data = self.validate_single_arg(args, "base58Encode")?;
        Ok(Base58::encode(data).into_bytes())
    }

    pub(super) fn base58_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let string_data = self.validate_string_arg(args, "base58Decode")?;
        Base58::decode(&string_data)
            .map_err(|e| Error::native_contract(format!("Invalid base58 data: {e}")))
    }

    pub(super) fn base58_check_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let data = self.validate_single_arg(args, "base58CheckEncode")?;
        Ok(Base58::encode_check(data).into_bytes())
    }

    pub(super) fn base58_check_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let string_data = self.validate_string_arg(args, "base58CheckDecode")?;
        Base58::decode_check(&string_data)
            .map_err(|e| Error::native_contract(format!("Invalid base58check data: {e}")))
    }

    pub(super) fn hex_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let data = self.validate_single_arg(args, "hexEncode")?;
        Ok(Hex::encode(data).into_bytes())
    }

    pub(super) fn hex_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        let string_data = self.validate_string_arg(args, "hexDecode")?;
        Hex::decode(&string_data)
            .map_err(|e| Error::native_contract(format!("Invalid hex data: {e}")))
    }
}
