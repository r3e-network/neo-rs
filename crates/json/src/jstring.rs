use crate::error::{JsonError, JsonResult};
use crate::JToken;
use std::fmt;

/// Represents a JSON string
/// This matches the C# JString class
#[derive(Debug, Clone, PartialEq)]
pub struct JString {
    value: String,
}

impl JString {
    /// Creates a new JSON string
    pub fn new(value: String) -> Self {
        Self { value }
    }

    /// Creates a new JSON string from a string slice
    pub fn from_str(value: &str) -> Self {
        Self {
            value: value.to_string(),
        }
    }

    /// Gets the string value
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Gets the string value as a String
    pub fn to_string(&self) -> String {
        self.value.clone()
    }

    /// Sets the string value
    pub fn set_value(&mut self, value: String) {
        self.value = value;
    }

    /// Gets the length of the string
    pub fn len(&self) -> usize {
        self.value.len()
    }

    /// Checks if the string is empty
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Converts the JString to a JToken::String
    pub fn to_jtoken(self) -> JToken {
        JToken::String(self.value)
    }

    /// Creates a JString from a JToken::String
    pub fn from_jtoken(token: JToken) -> JsonResult<Self> {
        match token {
            JToken::String(value) => Ok(Self { value }),
            _ => Err(JsonError::InvalidCast("Token is not a string".to_string())),
        }
    }
}

impl Default for JString {
    fn default() -> Self {
        Self::new(String::new())
    }
}

impl From<String> for JString {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for JString {
    fn from(value: &str) -> Self {
        Self::from_str(value)
    }
}

impl From<JString> for String {
    fn from(jstring: JString) -> Self {
        jstring.value
    }
}

impl AsRef<str> for JString {
    fn as_ref(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for JString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jstring_new() {
        let jstr = JString::new("test".to_string());
        assert_eq!(jstr.value(), "test");
        assert_eq!(jstr.len(), 4);
        assert!(!jstr.is_empty());
    }

    #[test]
    fn test_jstring_from_str() {
        let jstr = JString::from_str("hello");
        assert_eq!(jstr.value(), "hello");
        assert_eq!(jstr.to_string(), "hello".to_string());
    }

    #[test]
    fn test_jstring_empty() {
        let jstr = JString::default();
        assert!(jstr.is_empty());
        assert_eq!(jstr.len(), 0);
    }

    #[test]
    fn test_jstring_set_value() {
        let mut jstr = JString::new("initial".to_string());
        jstr.set_value("updated".to_string());
        assert_eq!(jstr.value(), "updated");
    }

    #[test]
    fn test_jstring_conversions() {
        let jstr = JString::from("test");
        let string: String = jstr.clone().into();
        assert_eq!(string, "test");

        let jstr2: JString = "hello".into();
        assert_eq!(jstr2.value(), "hello");
    }

    #[test]
    fn test_jstring_jtoken_conversion() {
        let jstr = JString::from("test");
        let token = jstr.clone().to_jtoken();

        match token {
            JToken::String(value) => assert_eq!(value, "test"),
            _ => panic!("Expected JToken::String"),
        }

        let jstr2 = JString::from_jtoken(JToken::String("hello".to_string())).unwrap();
        assert_eq!(jstr2.value(), "hello");

        let result = JString::from_jtoken(JToken::Number(42.0));
        assert!(result.is_err());
    }

    #[test]
    fn test_jstring_display() {
        let jstr = JString::from("test");
        assert_eq!(format!("{}", jstr), "test");
    }
}
