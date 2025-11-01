//! JNumber - matches C# Neo.Json.JNumber exactly

use std::io::Write;

/// Represents a JSON number (matches C# JNumber)
#[derive(Clone, Debug)]
pub struct JNumber {
    /// The value of the JSON token
    pub value: f64,
}

/// Largest safe integer in JSON
pub const MAX_SAFE_INTEGER: i64 = (1i64 << 53) - 1;

/// Smallest safe integer in JSON
pub const MIN_SAFE_INTEGER: i64 = -MAX_SAFE_INTEGER;

impl JNumber {
    /// Initializes a new instance with the specified value
    pub fn new(value: f64) -> Result<Self, String> {
        if !value.is_finite() {
            return Err("FormatException: Value must be finite".to_string());
        }
        Ok(Self { value })
    }

    /// Converts to boolean (true if not zero)
    pub fn as_boolean(&self) -> bool {
        self.value != 0.0
    }

    /// Gets the number value
    pub fn as_number(&self) -> f64 {
        self.value
    }

    /// Converts to string
    pub fn as_string(&self) -> String {
        self.value.to_string()
    }

    /// Gets the number value
    pub fn get_number(&self) -> f64 {
        self.value
    }

    /// Converts to string
    pub fn to_string(&self) -> String {
        self.as_string()
    }

    /// Writes to a JSON writer
    pub fn write(&self, writer: &mut dyn Write) -> std::io::Result<()> {
        writer.write_all(self.as_string().as_bytes())
    }

    /// Clones the token
    pub fn clone(&self) -> Self {
        Self { value: self.value }
    }
}

impl TryFrom<f64> for JNumber {
    type Error = String;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        JNumber::new(value)
    }
}

impl TryFrom<i64> for JNumber {
    type Error = String;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        JNumber::new(value as f64)
    }
}

impl PartialEq for JNumber {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for JNumber {}

impl std::hash::Hash for JNumber {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.to_bits().hash(state);
    }
}

impl std::fmt::Display for JNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_string())
    }
}
