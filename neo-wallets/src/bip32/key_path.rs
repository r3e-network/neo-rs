//! BIP-32 key path parser.

use neo_error::{CoreError, CoreResult};
use std::fmt;

/// Parsed BIP-32 derivation path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyPath {
    indices: Vec<u32>,
}

impl KeyPath {
    /// Return the master path (`m`) with no child indices.
    pub fn master() -> Self {
        Self {
            indices: Vec::new(),
        }
    }

    /// Return the child indices in derivation order.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Return a new path with `index` appended.
    pub fn derive(&self, index: u32) -> Self {
        let mut new_indices = self.indices.clone();
        new_indices.push(index);
        Self {
            indices: new_indices,
        }
    }

    /// Parse a BIP-32 path string such as `m/44'/888'/0'/0/0`.
    pub fn parse(path: &str) -> CoreResult<Self> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err(CoreError::other("Invalid key path"));
        }

        let mut parts = trimmed.split('/');
        let first = parts
            .next()
            .ok_or_else(|| CoreError::other("Invalid key path"))?;
        if first.trim() != "m" {
            return Err(CoreError::other("Invalid key path"));
        }

        let mut indices = Vec::new();
        for part in parts {
            let part = part.trim();
            if part.is_empty() {
                return Err(CoreError::other("Invalid key path"));
            }

            let hardened = part.ends_with('\'');
            let number_str = if hardened {
                &part[..part.len() - 1]
            } else {
                part
            };

            if number_str.is_empty() {
                return Err(CoreError::other("Invalid key path"));
            }

            let mut index: u32 = number_str
                .parse()
                .map_err(|_| CoreError::other("Invalid key path"))?;
            if index >= 0x8000_0000 {
                return Err(CoreError::other("Invalid key path"));
            }
            if hardened {
                index |= 0x8000_0000;
            }
            indices.push(index);
        }

        Ok(Self { indices })
    }
}

impl fmt::Display for KeyPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "m")?;
        for index in &self.indices {
            write!(f, "/")?;
            if (index & 0x8000_0000) != 0 {
                write!(f, "{}'", index & 0x7fff_ffff)?;
            } else {
                write!(f, "{}", index)?;
            }
        }
        Ok(())
    }
}
