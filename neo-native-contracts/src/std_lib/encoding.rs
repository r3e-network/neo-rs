//! StdLib byte/string encoding helpers.

use neo_crypto::{base58, base64};
use neo_error::{CoreError, CoreResult};

use super::{StdLib, args::MAX_INPUT_LENGTH};

pub(super) fn base64_encode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    StdLib::arg_bytes_max(args, "base64Encode", "data")
        .map(|bytes| base64::encode(bytes).into_bytes())
}

/// C# `StdLib.Base64Decode` = `Convert.FromBase64String`: strip the four
/// whitespace characters .NET tolerates ({space, `\t`, `\n`, `\r`}), then
/// strict-decode the remainder (any other character, including other
/// whitespace, faults). Enforces the C# `[MaxLength(1024)]` cap on the input.
pub(super) fn base64_decode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let raw = StdLib::arg_bytes(args, "base64Decode")?;
    if raw.len() > MAX_INPUT_LENGTH {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::base64Decode: input exceeds maximum length ({MAX_INPUT_LENGTH})"
        )));
    }
    let value = std::str::from_utf8(raw).map_err(|_| {
        CoreError::invalid_operation("StdLib::base64Decode: argument is not valid UTF-8")
    })?;
    let stripped: String = value
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\r'))
        .collect();
    base64::decode_strict(&stripped)
        .map_err(|e| CoreError::invalid_operation(format!("StdLib::base64Decode: {e}")))
}

/// C# `StdLib.Base64UrlEncode(data)` (HF_Echidna) =
/// `Base64UrlEncoder.Encode`: encodes the UTF-8 bytes of the input string into
/// a URL-safe, unpadded base64 string.
pub(super) fn base64_url_encode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let value = StdLib::arg_str(args, "base64UrlEncode")?;
    if value.len() > MAX_INPUT_LENGTH {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::base64UrlEncode: input exceeds maximum length ({MAX_INPUT_LENGTH})"
        )));
    }
    Ok(base64::url_encode_no_pad(value.as_bytes()).into_bytes())
}

/// C# `StdLib.Base64UrlDecode(s)` (HF_Echidna) =
/// `Base64UrlEncoder.Decode`: strip .NET-tolerated whitespace, then strict
/// URL-safe-no-padding decode.
pub(super) fn base64_url_decode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let raw = StdLib::arg_bytes(args, "base64UrlDecode")?;
    if raw.len() > MAX_INPUT_LENGTH {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::base64UrlDecode: input exceeds maximum length ({MAX_INPUT_LENGTH})"
        )));
    }
    let value = std::str::from_utf8(raw).map_err(|_| {
        CoreError::invalid_operation("StdLib::base64UrlDecode: argument is not valid UTF-8")
    })?;
    let stripped: String = value
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\r'))
        .collect();
    let decoded = base64::url_decode_no_pad_strict(&stripped)
        .map_err(|e| CoreError::invalid_operation(format!("StdLib::base64UrlDecode: {e}")))?;
    Ok(String::from_utf8_lossy(&decoded).into_owned().into_bytes())
}

pub(super) fn base58_encode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    StdLib::arg_bytes_max(args, "base58Encode", "data")
        .map(|bytes| base58::encode(bytes).into_bytes())
}

pub(super) fn base58_check_encode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    StdLib::arg_bytes_max(args, "base58CheckEncode", "data")
        .map(|bytes| base58::encode_check(bytes).into_bytes())
}

pub(super) fn base58_decode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    StdLib::arg_str_max(args, "base58Decode", "s").and_then(|text| {
        base58::decode(&text)
            .map_err(|e| CoreError::invalid_operation(format!("StdLib::base58Decode: {e}")))
    })
}

pub(super) fn base58_check_decode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    StdLib::arg_str_max(args, "base58CheckDecode", "s").and_then(|text| {
        base58::decode_check(&text)
            .map_err(|e| CoreError::invalid_operation(format!("StdLib::base58CheckDecode: {e}")))
    })
}

/// C# `StdLib.HexEncode(bytes)` (HF_Faun) = `bytes.ToHexString()`:
/// lowercase hex, no prefix.
pub(super) fn hex_encode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let raw = StdLib::arg_bytes(args, "hexEncode")?;
    if raw.len() > MAX_INPUT_LENGTH {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::hexEncode: input exceeds maximum length ({MAX_INPUT_LENGTH})"
        )));
    }
    Ok(hex::encode(raw).into_bytes())
}

/// C# `StdLib.HexDecode(str)` (HF_Faun) = `str.HexToBytes()`
/// (`Convert.FromHexString`): case-insensitive hex, even length, no prefix.
pub(super) fn hex_decode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let raw = StdLib::arg_bytes(args, "hexDecode")?;
    if raw.len() > MAX_INPUT_LENGTH {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::hexDecode: input exceeds maximum length ({MAX_INPUT_LENGTH})"
        )));
    }
    let value = std::str::from_utf8(raw).map_err(|_| {
        CoreError::invalid_operation("StdLib::hexDecode: argument is not valid UTF-8")
    })?;
    hex::decode(value).map_err(|e| CoreError::invalid_operation(format!("StdLib::hexDecode: {e}")))
}
