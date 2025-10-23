// Copyright (C) 2015-2025 The Neo Project.
//
// string_extensions.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::str;

/// String extensions matching C# StringExtensions exactly
pub trait StringExtensions {
    /// Converts a byte span to a strict UTF8 string.
    /// Matches C# TryToStrictUtf8String method
    fn try_to_strict_utf8_string(&self) -> Result<String, String>;

    /// Converts a byte span to a strict UTF8 string.
    /// Matches C# ToStrictUtf8String method
    fn to_strict_utf8_string(&self) -> Result<String, String>;

    /// Converts a string to a strict UTF8 byte array.
    /// Matches C# ToStrictUtf8Bytes method
    fn to_strict_utf8_bytes(&self) -> Result<Vec<u8>, String>;

    /// Gets the size of the specified string encoded in strict UTF8.
    /// Matches C# GetStrictUtf8ByteCount method
    fn get_strict_utf8_byte_count(&self) -> Result<usize, String>;

    /// Determines if the specified string is a valid hex string.
    /// Matches C# IsHex method
    fn is_hex(&self) -> bool;

    /// Converts a hex string to byte array.
    /// Matches C# HexToBytes method
    fn hex_to_bytes(&self) -> Result<Vec<u8>, String>;

    /// Converts a hex string to byte array then reverses the order of the bytes.
    /// Matches C# HexToBytesReversed method
    fn hex_to_bytes_reversed(&self) -> Result<Vec<u8>, String>;

    /// Gets the size of the specified string encoded in variable-length encoding.
    /// Matches C# GetVarSize method
    fn get_var_size(&self) -> Result<usize, String>;

    /// Trims the specified prefix from the start of the string, ignoring case.
    /// Matches C# TrimStartIgnoreCase method
    fn trim_start_ignore_case(&self, prefix: &str) -> String;
}

impl StringExtensions for &[u8] {
    fn try_to_strict_utf8_string(&self) -> Result<String, String> {
        match str::from_utf8(self) {
            Ok(s) => Ok(s.to_string()),
            Err(_) => Err("Invalid UTF-8 sequence".to_string()),
        }
    }

    fn to_strict_utf8_string(&self) -> Result<String, String> {
        match str::from_utf8(self) {
            Ok(s) => Ok(s.to_string()),
            Err(e) => {
                let bytes_info = if self.len() <= 32 {
                    format!(
                        "Bytes: [{}]",
                        self.iter()
                            .map(|b| format!("0x{:02X}", b))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                } else {
                    format!("Length: {} bytes", self.len())
                };
                Err(format!("Failed to decode byte span to UTF-8 string (strict mode): The input contains invalid UTF-8 byte sequences. {}. Ensure all bytes form valid UTF-8 character sequences.", bytes_info))
            }
        }
    }

    fn to_strict_utf8_bytes(&self) -> Result<Vec<u8>, String> {
        Ok(self.to_vec())
    }

    fn get_strict_utf8_byte_count(&self) -> Result<usize, String> {
        Ok(self.len())
    }

    fn is_hex(&self) -> bool {
        false // Not applicable for byte slices
    }

    fn hex_to_bytes(&self) -> Result<Vec<u8>, String> {
        Err("Not applicable for byte slices".to_string())
    }

    fn hex_to_bytes_reversed(&self) -> Result<Vec<u8>, String> {
        Err("Not applicable for byte slices".to_string())
    }

    fn get_var_size(&self) -> Result<usize, String> {
        let size = self.len();
        Ok(get_var_size(size) + size)
    }

    fn trim_start_ignore_case(&self, _prefix: &str) -> String {
        String::new() // Not applicable for byte slices
    }
}

impl StringExtensions for String {
    fn try_to_strict_utf8_string(&self) -> Result<String, String> {
        Ok(self.clone())
    }

    fn to_strict_utf8_string(&self) -> Result<String, String> {
        Ok(self.clone())
    }

    fn to_strict_utf8_bytes(&self) -> Result<Vec<u8>, String> {
        Ok(self.as_bytes().to_vec())
    }

    fn get_strict_utf8_byte_count(&self) -> Result<usize, String> {
        Ok(self.len())
    }

    fn is_hex(&self) -> bool {
        if self.is_empty() {
            return true;
        }

        if self.len() % 2 == 1 {
            return false;
        }

        for c in self.chars() {
            if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')) {
                return false;
            }
        }
        true
    }

    fn hex_to_bytes(&self) -> Result<Vec<u8>, String> {
        if self.is_empty() {
            return Ok(Vec::new());
        }

        match hex::decode(self) {
            Ok(bytes) => Ok(bytes),
            Err(_) => {
                let value_info = if self.len() <= 100 {
                    format!("Input: '{}'", self)
                } else {
                    format!("Input length: {} characters", self.len())
                };
                Err(format!("Failed to convert hex string to bytes: The input has an invalid length (must be even) or contains non-hexadecimal characters. {}. Valid hex characters are 0-9, A-F, and a-f.", value_info))
            }
        }
    }

    fn hex_to_bytes_reversed(&self) -> Result<Vec<u8>, String> {
        let mut bytes = self.hex_to_bytes()?;
        bytes.reverse();
        Ok(bytes)
    }

    fn get_var_size(&self) -> Result<usize, String> {
        let size = self.get_strict_utf8_byte_count()?;
        Ok(get_var_size(size) + size)
    }

    fn trim_start_ignore_case(&self, prefix: &str) -> String {
        if self.len() >= prefix.len() && self[..prefix.len()].eq_ignore_ascii_case(prefix) {
            self[prefix.len()..].to_string()
        } else {
            self.clone()
        }
    }
}

impl StringExtensions for &str {
    fn try_to_strict_utf8_string(&self) -> Result<String, String> {
        Ok(self.to_string())
    }

    fn to_strict_utf8_string(&self) -> Result<String, String> {
        Ok(self.to_string())
    }

    fn to_strict_utf8_bytes(&self) -> Result<Vec<u8>, String> {
        Ok(self.as_bytes().to_vec())
    }

    fn get_strict_utf8_byte_count(&self) -> Result<usize, String> {
        Ok(self.len())
    }

    fn is_hex(&self) -> bool {
        if self.is_empty() {
            return true;
        }

        if self.len() % 2 == 1 {
            return false;
        }

        for c in self.chars() {
            if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')) {
                return false;
            }
        }
        true
    }

    fn hex_to_bytes(&self) -> Result<Vec<u8>, String> {
        if self.is_empty() {
            return Ok(Vec::new());
        }

        match hex::decode(self) {
            Ok(bytes) => Ok(bytes),
            Err(_) => {
                let value_info = if self.len() <= 100 {
                    format!("Input: '{}'", self)
                } else {
                    format!("Input length: {} characters", self.len())
                };
                Err(format!("Failed to convert hex string to bytes: The input has an invalid length (must be even) or contains non-hexadecimal characters. {}. Valid hex characters are 0-9, A-F, and a-f.", value_info))
            }
        }
    }

    fn hex_to_bytes_reversed(&self) -> Result<Vec<u8>, String> {
        let mut bytes = self.hex_to_bytes()?;
        bytes.reverse();
        Ok(bytes)
    }

    fn get_var_size(&self) -> Result<usize, String> {
        let size = self.get_strict_utf8_byte_count()?;
        Ok(get_var_size(size) + size)
    }

    fn trim_start_ignore_case(&self, prefix: &str) -> String {
        if self.len() >= prefix.len() && self[..prefix.len()].eq_ignore_ascii_case(prefix) {
            self[prefix.len()..].to_string()
        } else {
            self.to_string()
        }
    }
}

/// Helper function to get variable size
/// Matches C# GetVarSize method for integers
fn get_var_size(value: usize) -> usize {
    if value < 0xfd {
        1
    } else if value <= 0xffff {
        3
    } else if value <= 0xffffffff {
        5
    } else {
        9
    }
}
