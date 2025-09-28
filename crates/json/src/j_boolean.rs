//! JBoolean - matches C# Neo.Json.JBoolean exactly

use std::io::Write;

/// Represents a JSON boolean value (matches C# JBoolean)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JBoolean {
    /// The value of the JSON token
    pub value: bool,
}

impl JBoolean {
    /// Initializes a new instance with the specified value
    pub fn new(value: bool) -> Self {
        Self { value }
    }

    /// Converts to boolean
    pub fn as_boolean(&self) -> bool {
        self.value
    }

    /// Converts to a floating point number (1 if true, 0 if false)
    pub fn as_number(&self) -> f64 {
        if self.value {
            1.0
        } else {
            0.0
        }
    }

    /// Converts to string
    pub fn as_string(&self) -> String {
        self.value.to_string().to_lowercase()
    }

    /// Gets the boolean value
    pub fn get_boolean(&self) -> bool {
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

impl From<bool> for JBoolean {
    fn from(value: bool) -> Self {
        JBoolean::new(value)
    }
}

impl std::fmt::Display for JBoolean {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_string())
    }
}
