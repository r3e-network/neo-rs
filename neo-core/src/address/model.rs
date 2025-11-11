use alloc::string::String;

#[derive(Debug, Clone)]
pub struct Address {
    version: u8,
    base58check: String,
}

impl Address {
    #[inline]
    pub fn new(version: u8, base58check: String) -> Self {
        Self {
            version,
            base58check,
        }
    }

    #[inline]
    pub fn version(&self) -> u8 {
        self.version
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        self.base58check.as_str()
    }
}

impl Into<String> for Address {
    fn into(self) -> String {
        self.base58check
    }
}
