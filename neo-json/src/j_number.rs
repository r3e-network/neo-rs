//! `JNumber` - matches C# Neo.Json.JNumber exactly

use std::io::Write;

/// Represents a JSON number (matches C# `JNumber`)
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
    ///
    /// # Errors
    ///
    /// Returns `String` error if the value is not finite.
    pub fn new(value: f64) -> Result<Self, String> {
        if !value.is_finite() {
            return Err("FormatException: Value must be finite".to_string());
        }
        Ok(Self { value })
    }

    /// Converts to boolean (true if not zero)
    #[must_use]
    pub fn as_boolean(&self) -> bool {
        self.value != 0.0
    }

    /// Gets the number value
    #[must_use]
    pub const fn as_number(&self) -> f64 {
        self.value
    }

    /// Converts to string
    #[must_use]
    pub fn as_string(&self) -> String {
        self.value.to_string()
    }

    /// Gets the number value
    #[must_use]
    pub const fn get_number(&self) -> f64 {
        self.value
    }

    /// Writes to a JSON writer
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if writing fails.
    pub fn write(&self, writer: &mut dyn Write) -> std::io::Result<()> {
        writer.write_all(self.as_string().as_bytes())
    }
}

impl TryFrom<f64> for JNumber {
    type Error = String;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<i64> for JNumber {
    type Error = String;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Self::new(value as f64)
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
