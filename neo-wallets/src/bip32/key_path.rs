//! BIP-32 key path parser.

use std::fmt;

/// A parsed BIP-32 key derivation path, such as `m/44'/888'/0'/0/0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyPath {
    indices: Vec<u32>,
}

impl KeyPath {
    /// Creates the master key path (`m`) with no derivation indices.
    pub fn master() -> Self {
        Self {
            indices: Vec::new(),
        }
    }

    /// Returns the derivation indices of this path, with the hardening bit set on hardened indices.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Returns a new path extended by the given child index.
    pub fn derive(&self, index: u32) -> Self {
        let mut new_indices = self.indices.clone();
        new_indices.push(index);
        Self {
            indices: new_indices,
        }
    }

    /// Parses a BIP-32 path string such as `m/44'/0'/0'`; hardened components are suffixed with `'`.
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
                write!(f, "{index}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::KeyPath;

    #[test]
    fn test_vectors() {
        let cases: &[(&str, &[u32])] = &[
            ("m", &[]),
            ("m/0'", &[0x8000_0000]),
            ("m/0'/1", &[0x8000_0000, 1]),
            ("m/0'/1/2'", &[0x8000_0000, 1, 0x8000_0002]),
            ("m/0'/1/2'/2", &[0x8000_0000, 1, 0x8000_0002, 2]),
            (
                "m/0'/1/2'/2/1000000000",
                &[0x8000_0000, 1, 0x8000_0002, 2, 1_000_000_000],
            ),
            ("m", &[]),
            ("m/0", &[0]),
            ("m/0/2147483647'", &[0, 0xffff_ffff]),
            ("m/0/2147483647'/1", &[0, 0xffff_ffff, 1]),
            (
                "m/0/2147483647'/1/2147483646'",
                &[0, 0xffff_ffff, 1, 0xffff_fffe],
            ),
            (
                "m/0/2147483647'/1/2147483646'/2",
                &[0, 0xffff_ffff, 1, 0xffff_fffe, 2],
            ),
            ("m", &[]),
            ("m/0'", &[0x8000_0000]),
            ("m", &[]),
            ("m/0'", &[0x8000_0000]),
            ("m/0'/1'", &[0x8000_0000, 0x8000_0001]),
        ];

        for (path, indices) in cases {
            let parsed = KeyPath::parse(path).unwrap();
            assert_eq!(parsed.indices(), *indices, "path {path}");
            assert_eq!(parsed.to_string(), *path, "path {path}");
        }
    }

    #[test]
    fn master_and_derive_preserve_indices() {
        let master = KeyPath::master();
        assert!(master.indices().is_empty());
        assert_eq!(master.to_string(), "m");
        assert_eq!(KeyPath::parse("m").unwrap(), master);

        let child = master.derive(0);
        let hardened = child.derive(0x8000_0001);
        assert!(master.indices().is_empty());
        assert_eq!(child.indices(), &[0]);
        assert_eq!(hardened.indices(), &[0, 0x8000_0001]);
        assert_eq!(child.to_string(), "m/0");
        assert_eq!(hardened.to_string(), "m/0/1'");
        assert_eq!(hardened.clone(), hardened);

        let maximal = master.derive(u32::MAX);
        assert_eq!(maximal.indices(), &[u32::MAX]);
        assert_eq!(maximal.to_string(), "m/2147483647'");
    }

    #[test]
    fn parse_accepts_existing_whitespace_and_numeric_forms() {
        let cases: &[(&str, &[u32], &str)] = &[
            (" \t m \n ", &[], "m"),
            (" \t m / +0001 / 0002' \n ", &[1, 0x8000_0002], "m/1/2'"),
            (
                "m/2147483647/2147483647'",
                &[0x7fff_ffff, 0xffff_ffff],
                "m/2147483647/2147483647'",
            ),
            ("m/+0'/0000", &[0x8000_0000, 0], "m/0'/0"),
        ];

        for (path, indices, canonical) in cases {
            let parsed = KeyPath::parse(path).unwrap();
            assert_eq!(parsed.indices(), *indices, "path {path}");
            assert_eq!(parsed.to_string(), *canonical, "path {path}");
        }
    }

    #[test]
    fn invalid_key_paths_return_existing_error() {
        for path in [
            "",
            " ",
            "M",
            "0",
            "/m",
            "m/",
            "m//0",
            "m/'",
            "m/0''",
            "m/-1",
            "m/0h",
            "m/0H",
            "m/0x1",
            "m/1 '",
            "m/2147483648",
            "m/2147483648'",
            "m/4294967295",
            "m/4294967296",
            "m/+",
            "m/1/ /2",
            "m/1/2/",
            "m/0/garbage",
        ] {
            assert_eq!(
                KeyPath::parse(path).unwrap_err(),
                "Invalid key path",
                "path {path}"
            );
        }
    }
}
