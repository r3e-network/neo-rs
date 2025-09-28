// Copyright (C) 2015-2025 The Neo Project.
//
// secure_string_extensions.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// Secure string extensions matching C# SecureStringExtensions exactly
pub trait SecureStringExtensions {
    /// Gets the clear text from a secure string.
    /// Matches C# GetClearText method
    fn get_clear_text(&self) -> Result<String, String>;
    
    /// Converts a string to a secure string.
    /// Matches C# ToSecureString method
    fn to_secure_string(&self, as_read_only: bool) -> SecureString;
}

/// Secure string structure
pub struct SecureString {
    data: Vec<u8>,
    read_only: bool,
}

impl SecureString {
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            read_only: false,
        }
    }
    
    pub fn make_read_only(&mut self) {
        self.read_only = true;
    }
    
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }
}

impl SecureStringExtensions for SecureString {
    fn get_clear_text(&self) -> Result<String, String> {
        match String::from_utf8(self.data.clone()) {
            Ok(s) => Ok(s),
            Err(e) => Err(format!("Failed to convert secure string to clear text: {}", e)),
        }
    }
    
    fn to_secure_string(&self, _as_read_only: bool) -> SecureString {
        SecureString::new(self.data.clone())
    }
}

impl SecureStringExtensions for String {
    fn get_clear_text(&self) -> Result<String, String> {
        Ok(self.clone())
    }
    
    fn to_secure_string(&self, as_read_only: bool) -> SecureString {
        let mut secure = SecureString::new(self.as_bytes().to_vec());
        if as_read_only {
            secure.make_read_only();
        }
        secure
    }
}

impl SecureStringExtensions for &str {
    fn get_clear_text(&self) -> Result<String, String> {
        Ok(self.to_string())
    }
    
    fn to_secure_string(&self, as_read_only: bool) -> SecureString {
        let mut secure = SecureString::new(self.as_bytes().to_vec());
        if as_read_only {
            secure.make_read_only();
        }
        secure
    }
}