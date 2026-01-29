//! `JString` - matches C# Neo.Json.JString exactly

use std::io::Write;

/// Represents a JSON string (matches C# `JString`)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JString {
    /// The value of the JSON token
    pub value: String,
}

impl JString {
    /// Initializes a new instance with the specified value
    #[must_use]
    pub const fn new(value: String) -> Self {
        Self { value }
    }

    /// Converts to boolean (true if not empty)
    #[must_use]
    pub fn as_boolean(&self) -> bool {
        !self.value.is_empty()
    }

    /// Converts to number
    #[must_use]
    pub fn as_number(&self) -> f64 {
        if self.value.is_empty() {
            return 0.0;
        }
        self.value.parse::<f64>().unwrap_or(f64::NAN)
    }

    /// Gets the string value
    #[must_use]
    pub fn as_string(&self) -> String {
        self.value.clone()
    }

    /// Gets the string value
    #[must_use]
    pub fn get_string(&self) -> String {
        self.value.clone()
    }

    /// Writes to a JSON writer
    pub fn write(&self, writer: &mut dyn Write) -> std::io::Result<()> {
        // Write JSON-escaped string
        writer.write_all(b"\"")?;
        for ch in self.value.chars() {
            match ch {
                '"' => writer.write_all(b"\\\"")?,
                '\\' => writer.write_all(b"\\\\")?,
                '\n' => writer.write_all(b"\\n")?,
                '\r' => writer.write_all(b"\\r")?,
                '\t' => writer.write_all(b"\\t")?,
                '\u{0008}' => writer.write_all(b"\\b")?,
                '\u{000C}' => writer.write_all(b"\\f")?,
                c if c.is_control() => {
                    write!(writer, "\\u{:04x}", c as u32)?;
                }
                c => {
                    write!(writer, "{c}")?;
                }
            }
        }
        writer.write_all(b"\"")?;
        Ok(())
    }
}

impl From<String> for JString {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for JString {
    fn from(value: &str) -> Self {
        Self::new(value.to_string())
    }
}

impl std::fmt::Display for JString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}
