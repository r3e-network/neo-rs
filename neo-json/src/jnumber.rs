// Copyright (C) 2015-2024 The Neo Project.
//
// jnumber.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::cmp::Ordering;
use std::convert::TryFrom;
use std::fmt;
use serde_json::ser::Serializer;
use crate::jtoken::JToken;

/// Represents a JSON number.
#[derive(Debug, Clone, Copy)]
pub struct JNumber {
    /// The value of the JSON token.
    pub value: f64,
}

impl JNumber {
    /// Represents the largest safe integer in JSON.
    pub const MAX_SAFE_INTEGER: i64 = (1 << 53) - 1;

    /// Represents the smallest safe integer in JSON.
    pub const MIN_SAFE_INTEGER: i64 = -Self::MAX_SAFE_INTEGER;

    /// Initializes a new instance of the `JNumber` struct with the specified value.
    ///
    /// # Arguments
    ///
    /// * `value` - The value of the JSON token.
    ///
    /// # Panics
    ///
    /// Panics if the value is not finite.
    pub fn new(value: f64) -> Self {
        if !value.is_finite() {
            panic!("Value must be finite");
        }
        Self { value }
    }

    /// Converts the current JSON token to a boolean value.
    pub fn as_boolean(&self) -> bool {
        self.value != 0.0
    }

    /// Converts the current JSON token to a number.
    pub fn as_number(&self) -> f64 {
        self.value
    }

    /// Converts the current JSON token to a string.
    pub fn as_string(&self) -> String {
        self.value.to_string()
    }

    /// Gets the number value.
    pub fn get_number(&self) -> f64 {
        self.value
    }

    /// Attempts to convert the JSON number to an enum value.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The enum type to convert to.
    ///
    /// # Arguments
    ///
    /// * `default_value` - The default value to return if conversion fails.
    /// * `ignore_case` - Ignored in this implementation as Rust enums are case-sensitive.
    ///
    /// # Returns
    ///
    /// The enum value if conversion succeeds, otherwise the default value.
    pub fn as_enum<T: TryFrom<i64>>(&self, default_value: T, _ignore_case: bool) -> T {
        T::try_from(self.value as i64).unwrap_or(default_value)
    }

    /// Converts the JSON number to an enum value.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The enum type to convert to.
    ///
    /// # Arguments
    ///
    /// * `ignore_case` - Ignored in this implementation as Rust enums are case-sensitive.
    ///
    /// # Returns
    ///
    /// The enum value.
    ///
    /// # Panics
    ///
    /// Panics if the conversion fails.
    pub fn get_enum<T: TryFrom<i64>>(&self, _ignore_case: bool) -> T {
        T::try_from(self.value as i64).expect("Failed to convert to enum")
    }

    /// Writes the JSON representation of the number.
    pub fn write<W: std::io::Write>(&self, writer: &mut Serializer<W>) -> Result<(), serde_json::Error> {
        writer.serialize_f64(self.value)
    }
}

impl JToken for JNumber {
    fn clone(&self) -> Box<dyn JToken> {
        Box::new(*self)
    }

    // Implement other JToken methods as needed
}

impl From<f64> for JNumber {
    fn from(value: f64) -> Self {
        JNumber::new(value)
    }
}

impl From<i64> for JNumber {
    fn from(value: i64) -> Self {
        JNumber::new(value as f64)
    }
}

impl PartialEq for JNumber {
    fn eq(&self, other: &Self) -> bool {
        self.value.eq(&other.value)
    }
}

impl Eq for JNumber {}

impl PartialOrd for JNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.value.partial_cmp(&other.value)
    }
}

impl Ord for JNumber {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}

impl fmt::Display for JNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl PartialEq<f64> for JNumber {
    fn eq(&self, other: &f64) -> bool {
        self.value.eq(other)
    }
}

impl PartialEq<i64> for JNumber {
    fn eq(&self, other: &i64) -> bool {
        self.value.eq(&(*other as f64))
    }
}

// Implement PartialEq for other numeric types as needed

impl std::hash::Hash for JNumber {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.to_bits().hash(state);
    }
}
