
use std::str::FromStr;
use std::fmt;
use serde_json::ser::Serializer;
use crate::jtoken::JToken;

/// Represents a JSON string.
#[derive(Clone, Debug)]
pub struct JString {
    /// The value of the JSON token.
    pub value: String,
}

impl JString {
    /// Initializes a new instance of the JString struct with the specified value.
    ///
    /// # Arguments
    ///
    /// * `value` - The value of the JSON token.
    ///
    /// # Panics
    ///
    /// Panics if the value is None.
    pub fn new(value: String) -> Self {
        Self { value }
    }

    /// Converts the current JSON token to a boolean value.
    ///
    /// # Returns
    ///
    /// `true` if value is not empty; otherwise, `false`.
    pub fn as_boolean(&self) -> bool {
        !self.value.is_empty()
    }

    /// Converts the current JSON token to a number.
    pub fn as_number(&self) -> f64 {
        if self.value.is_empty() {
            return 0.0;
        }
        self.value.parse::<f64>().unwrap_or(f64::NAN)
    }

    /// Converts the current JSON token to a string.
    pub fn as_string(&self) -> String {
        self.value.clone()
    }

    /// Gets the string value.
    pub fn get_string(&self) -> &str {
        &self.value
    }

    /// Attempts to convert the JSON string to an enum value.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The enum type to convert to.
    ///
    /// # Arguments
    ///
    /// * `default_value` - The default value to return if conversion fails.
    /// * `ignore_case` - Whether to ignore case when parsing the enum.
    ///
    /// # Returns
    ///
    /// The parsed enum value or the default value if parsing fails.
    pub fn as_enum<T: FromStr>(&self, default_value: T, ignore_case: bool) -> T
    where
        T: Default,
    {
        let parse_result = if ignore_case {
            T::from_str(&self.value.to_lowercase())
        } else {
            T::from_str(&self.value)
        };
        parse_result.unwrap_or(default_value)
    }

    /// Gets the enum value.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The enum type to convert to.
    ///
    /// # Arguments
    ///
    /// * `ignore_case` - Whether to ignore case when parsing the enum.
    ///
    /// # Returns
    ///
    /// The parsed enum value.
    ///
    /// # Panics
    ///
    /// Panics if the conversion fails.
    pub fn get_enum<T: FromStr>(&self, ignore_case: bool) -> T
    where
        T: Default,
    {
        let parse_result = if ignore_case {
            T::from_str(&self.value.to_lowercase())
        } else {
            T::from_str(&self.value)
        };
        parse_result.expect("Failed to parse enum")
    }

    /// Writes the JSON string to a JSON writer.
    pub fn write<W: fmt::Write>(&self, writer: &mut W) -> fmt::Result {
        writer.write_str(&format!("\"{}\"", self.value))
    }

    /// Clones the JSON token.
    pub fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
        }
    }
}

impl From<&str> for JString {
    fn from(value: &str) -> Self {
        JString::new(value.to_string())
    }
}

impl From<String> for JString {
    fn from(value: String) -> Self {
        JString::new(value)
    }
}

impl PartialEq for JString {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for JString {}

impl PartialEq<str> for JString {
    fn eq(&self, other: &str) -> bool {
        self.value == other
    }
}

impl PartialEq<String> for JString {
    fn eq(&self, other: &String) -> bool {
        &self.value == other
    }
}

impl fmt::Display for JString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl JToken for JString {
    fn as_boolean(&self) -> bool {
        self.as_boolean()
    }

    fn as_number(&self) -> f64 {
        self.as_number()
    }

    fn as_string(&self) -> String {
        self.as_string()
    }

    fn clone_token(&self) -> Box<dyn JToken> {
        Box::new(self.clone())
    }
}
