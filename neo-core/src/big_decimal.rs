use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use num_bigint::BigInt;
use num_traits::{Signed, Zero};

/// Represents a fixed-point number of arbitrary precision.
#[derive(Clone, Debug)]
pub struct BigDecimal {
    value: BigInt,
    decimals: u8,
}

impl BigDecimal {
    /// Creates a new `BigDecimal` instance.
    ///
    /// # Arguments
    ///
    /// * `value` - The `BigInt` value of the number.
    /// * `decimals` - The number of decimal places for this number.
    pub fn new(value: BigInt, decimals: u8) -> Self {
        Self { value, decimals }
    }

    /// The `BigInt` value of the number.
    pub fn value(&self) -> &BigInt {
        &self.value
    }

    /// The number of decimal places for this number.
    pub fn decimals(&self) -> u8 {
        self.decimals
    }

    /// The sign of the number.
    pub fn sign(&self) -> i32 {
        self.value.sign() as i32
    }

    /// Changes the decimals of the `BigDecimal`.
    ///
    /// # Arguments
    ///
    /// * `decimals` - The new decimals field.
    ///
    /// # Returns
    ///
    /// The `BigDecimal` that has the new number of decimal places.
    pub fn change_decimals(&self, decimals: u8) -> Result<Self, &'static str> {
        if self.decimals == decimals {
            return Ok(self.clone());
        }
        let value = if self.decimals < decimals {
            &self.value * BigInt::from(10).pow((decimals - self.decimals) as u32)
        } else {
            let divisor = BigInt::from(10).pow((self.decimals - decimals) as u32);
            let (quotient, remainder) = self.value.div_rem(&divisor);
            if !remainder.is_zero() {
                return Err("Cannot change decimals without losing precision");
            }
            quotient
        };
        Ok(Self::new(value, decimals))
    }

    /// Parses a `BigDecimal` from the specified string.
    ///
    /// # Arguments
    ///
    /// * `s` - A number represented by a string.
    /// * `decimals` - The number of decimal places for this number.
    ///
    /// # Returns
    ///
    /// The parsed `BigDecimal`.
    pub fn parse(s: &str, decimals: u8) -> Result<Self, &'static str> {
        Self::try_parse(s, decimals).ok_or("Failed to parse BigDecimal")
    }

    /// Attempts to parse a `BigDecimal` from the specified string.
    ///
    /// # Arguments
    ///
    /// * `s` - A number represented by a string.
    /// * `decimals` - The number of decimal places for this number.
    ///
    /// # Returns
    ///
    /// An `Option` containing the parsed `BigDecimal` if successful, or `None` if parsing failed.
    pub fn try_parse(s: &str, decimals: u8) -> Option<Self> {
        let mut e = 0i32;
        let mut s = s.to_string();
        if let Some(index) = s.find(['e', 'E']) {
            e = s[index + 1..].parse().ok()?;
            s.truncate(index);
        }
        if let Some(index) = s.find('.') {
            s = s.trim_end_matches('0').to_string();
            e -= (s.len() - index - 1) as i32;
            s = s.replace(".", "");
        }
        let ds = e + decimals as i32;
        if ds < 0 {
            return None;
        }
        if ds > 0 {
            s.push_str(&"0".repeat(ds as usize));
        }
        let value = BigInt::from_str(&s).ok()?;
        Some(Self::new(value, decimals))
    }
}

impl fmt::Display for BigDecimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let divisor = BigInt::from(10).pow(self.decimals as u32);
        let (result, remainder) = self.value.div_rem(&divisor);
        if remainder.is_zero() {
            write!(f, "{}", result)
        } else {
            write!(f, "{}.{}", result, remainder.abs().to_string().trim_end_matches('0'))
        }
    }
}

impl PartialEq for BigDecimal {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for BigDecimal {}

impl PartialOrd for BigDecimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BigDecimal {
    fn cmp(&self, other: &Self) -> Ordering {
        let (left, right) = if self.decimals < other.decimals {
            (
                &self.value * BigInt::from(10).pow((other.decimals - self.decimals) as u32),
                other.value.clone(),
            )
        } else if self.decimals > other.decimals {
            (
                self.value.clone(),
                &other.value * BigInt::from(10).pow((self.decimals - other.decimals) as u32),
            )
        } else {
            (self.value.clone(), other.value.clone())
        };
        left.cmp(&right)
    }
}

impl Hash for BigDecimal {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let divisor = BigInt::from(10).pow(self.decimals as u32);
        let (result, remainder) = self.value.div_rem(&divisor);
        result.hash(state);
        remainder.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_big_decimal_creation() {
        let bd = BigDecimal::new(BigInt::from(1234), 2);
        assert_eq!(bd.value(), &BigInt::from(1234));
        assert_eq!(bd.decimals(), 2);
    }

    #[test]
    fn test_big_decimal_display() {
        let bd = BigDecimal::new(BigInt::from(123456), 2);
        assert_eq!(bd.to_string(), "1234.56");
    }

    #[test]
    fn test_big_decimal_comparison() {
        let bd1 = BigDecimal::new(BigInt::from(123456), 2);
        let bd2 = BigDecimal::new(BigInt::from(123456), 2);
        let bd3 = BigDecimal::new(BigInt::from(123457), 2);
        assert_eq!(bd1, bd2);
        assert!(bd1 < bd3);
    }

    #[test]
    fn test_big_decimal_parse() {
        let bd = BigDecimal::parse("1234.56", 2).unwrap();
        assert_eq!(bd.value(), &BigInt::from(123456));
        assert_eq!(bd.decimals(), 2);
    }

    #[test]
    fn test_big_decimal_change_decimals() {
        let bd = BigDecimal::new(BigInt::from(123456), 2);
        let changed = bd.change_decimals(3).unwrap();
        assert_eq!(changed.value(), &BigInt::from(1234560));
        assert_eq!(changed.decimals(), 3);
    }
}
