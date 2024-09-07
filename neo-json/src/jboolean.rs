
use std::fmt;
use serde_json::ser::Serializer;

/// Represents a JSON boolean value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JBoolean {
    /// The value of the JSON token.
    pub value: bool,
}

impl JBoolean {
    /// Creates a new instance of the `JBoolean` struct with the specified value.
    ///
    /// # Arguments
    ///
    /// * `value` - The value of the JSON token.
    pub fn new(value: bool) -> Self {
        Self { value }
    }

    /// Converts the current JSON token to a boolean.
    pub fn as_boolean(&self) -> bool {
        self.value
    }

    /// Converts the current JSON token to a floating point number.
    ///
    /// Returns 1.0 if value is `true`; otherwise, 0.0.
    pub fn as_number(&self) -> f64 {
        if self.value { 1.0 } else { 0.0 }
    }

    /// Converts the current JSON token to a string.
    pub fn as_string(&self) -> String {
        self.value.to_string().to_lowercase()
    }

    /// Returns the boolean value.
    pub fn get_boolean(&self) -> bool {
        self.value
    }

    /// Clones the current JSON token.
    pub fn clone(&self) -> Self {
        *self
    }

    /// Writes the JSON representation of the boolean value.
    pub fn write<W: std::io::Write>(&self, writer: &mut Serializer<W>) -> Result<(), serde_json::Error> {
        writer.serialize_bool(self.value)
    }
}

impl From<bool> for JBoolean {
    fn from(value: bool) -> Self {
        JBoolean::new(value)
    }
}

impl PartialEq<bool> for JBoolean {
    fn eq(&self, other: &bool) -> bool {
        self.value == *other
    }
}

impl fmt::Display for JBoolean {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_string())
    }
}

impl std::hash::Hash for JBoolean {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}
