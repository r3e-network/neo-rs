use std::convert::TryFrom;
use num_bigint::{BigInt, BigUint, Sign};
use std::sync::OnceLock;
use encoding_rs::UTF_8;

static STRICT_UTF8: OnceLock<encoding_rs::Encoding> = OnceLock::new();

fn get_strict_utf8() -> &'static encoding_rs::Encoding {
    STRICT_UTF8.get_or_init(|| {
        let mut utf8 = UTF_8;
        utf8.set_fallback(None);
        utf8
    })
}

pub fn try_get_string(bytes: &[u8]) -> Option<String> {
    let (cow, _, had_errors) = get_strict_utf8().decode(bytes);
    if had_errors {
        None
    } else {
        Some(cow.into_owned())
    }
}

pub trait BigIntExt {
    fn mod_inverse(&self, modulus: &BigInt) -> Option<BigInt>;
    fn sqrt(&self) -> Option<BigInt>;
    fn get_bit_length(&self) -> u64;
}

impl BigIntExt for BigInt {
    fn mod_inverse(&self, modulus: &BigInt) -> Option<BigInt> {
        if self <= &BigInt::from(0) || modulus < &BigInt::from(2) {
            return None;
        }

        let (mut r, mut old_r) = (self.clone(), modulus.clone());
        let (mut s, mut old_s) = (BigInt::from(1), BigInt::from(0));

        while r > BigInt::from(0) {
            let q = &old_r / &r;
            let temp_r = old_r.clone();
            old_r = r.clone();
            r = temp_r - &q * &r;

            let temp_s = old_s.clone();
            old_s = s.clone();
            s = temp_s - &q * &s;
        }

        let mut result = old_s % modulus;
        if result < BigInt::from(0) {
            result += modulus;
        }

        if (self * &result) % modulus != BigInt::from(1) {
            None
        } else {
            Some(result)
        }
    }

    fn sqrt(&self) -> Option<BigInt> {
        if self < &BigInt::from(0) {
            return None;
        }
        if self == &BigInt::from(0) {
            return Some(BigInt::from(0));
        }
        if self < &BigInt::from(4) {
            return Some(BigInt::from(1));
        }

        let mut z = self.clone();
        let mut x = BigInt::from(1) << ((self.get_bit_length() as usize + 1) >> 1);

        while &x < &z {
            z = x.clone();
            x = (self / &x + &x) / 2;
        }

        Some(z)
    }

    fn get_bit_length(&self) -> u64 {
        if self == &BigInt::from(0) || self == &BigInt::from(-1) {
            return 0;
        }

        let (sign, bytes) = self.to_bytes_le();
        let len = bytes.len();

        if len == 1 || (len == 2 && bytes[1] == 0) {
            let byte = match sign {
                Sign::Plus => bytes[0],
                Sign::Minus => 255 - bytes[0],
                Sign::NoSign => return 0,
            };
            bit_count(byte as u32) as u64
        } else {
            let last_byte = match sign {
                Sign::Plus => bytes[len - 1],
                Sign::Minus => 255 - bytes[len - 1],
                Sign::NoSign => return 0,
            };
            ((len - 1) * 8) as u64 + bit_count(last_byte as u32) as u64
        }
    }
}

pub trait BigUintExt {
    fn mod_inverse(&self, m: &BigUint) -> Option<BigUint>;
    fn sqrt(&self) -> Option<BigUint>;
    fn get_bit_length(&self) -> u64;
}

impl BigUintExt for BigUint {
    fn mod_inverse(&self, m: &BigUint) -> Option<BigUint> {
        // Implementation for mod_inverse
        let (g, x, _) = self.extended_gcd(m);
        if g != BigUint::from(1u32) {
            None
        } else {
            Some((x % m + m) % m)
        }
    }

    fn sqrt(&self) -> Option<BigUint> {
        if self == &BigUint::from(0u32) {
            return Some(BigUint::from(0u32));
        }
        if self < &BigUint::from(4u32) {
            return Some(BigUint::from(1u32));
        }

        let mut z = self.clone();
        let mut x = BigUint::from(1u32) << ((self.get_bit_length() as usize + 1) >> 1);

        while &x < &z {
            z = x.clone();
            x = (self / &x + &x) / 2u32;
        }

        Some(z)
    }

    fn get_bit_length(&self) -> u64 {
        if self == &BigUint::from(0u32) {
            return 0;
        }

        let bytes = self.to_bytes_be();
        let len = bytes.len();
        let last_byte = bytes[0];

        ((len - 1) * 8) as u64 + bit_count(last_byte as u32) as u64
    }
}



#[inline]
fn bit_count(mut w: u32) -> u32 {
    w = w - ((w >> 1) & 0x55555555);
    w = (w & 0x33333333) + ((w >> 2) & 0x33333333);
    w = (w + (w >> 4)) & 0x0f0f0f0f;
    w = w + (w >> 8);
    w = w + (w >> 16);
    w & 0x3f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_get_string() {
        assert_eq!(try_get_string(b"Hello"), Some("Hello".to_string()));
        assert_eq!(try_get_string(&[0xFF]), None);
    }

    #[test]
    fn test_mod_inverse() {
        let a = BigInt::from(3);
        let m = BigInt::from(11);
        assert_eq!(a.mod_inverse(&m), Some(BigInt::from(4)));
    }

    #[test]
    fn test_sqrt() {
        let a = BigInt::from(16);
        assert_eq!(a.sqrt(), BigInt::from(4));
    }

    #[test]
    fn test_get_bit_length() {
        let a = BigInt::from(15);  // 1111 in binary
        assert_eq!(a.get_bit_length(), 4);
    }
}