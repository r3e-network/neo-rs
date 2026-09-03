//! BIP-32 key path parser.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyPath {
    indices: Vec<u32>,
}

impl KeyPath {
    pub fn master() -> Self {
        Self {
            indices: Vec::new(),
        }
    }

    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    pub fn derive(&self, index: u32) -> Self {
        let mut new_indices = self.indices.clone();
        new_indices.push(index);
        Self {
            indices: new_indices,
        }
    }

    pub fn parse(path: &str) -> Result<Self, String> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err("Invalid key path".to_string());
        }

        let mut parts = trimmed.split('/');
        let first = parts.next().ok_or_else(|| "Invalid key path".to_string())?;
        if first.trim() != "m" {
            return Err("Invalid key path".to_string());
        }

        let mut indices = Vec::new();
        for part in parts {
            let part = part.trim();
            if part.is_empty() {
                return Err("Invalid key path".to_string());
            }

            let hardened = part.ends_with('\'');
            let number_str = if hardened {
                &part[..part.len() - 1]
            } else {
                part
            };

            if number_str.is_empty() {
                return Err("Invalid key path".to_string());
            }

            let mut index: u32 = number_str
                .parse()
                .map_err(|_| "Invalid key path".to_string())?;
            if index >= 0x8000_0000 {
                return Err("Invalid key path".to_string());
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
