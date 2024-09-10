use crate::io::caching::*;
use std::cmp::Ordering;
use std::fmt;
use std::io::{Read, Write};
use num_bigint::BigInt;
use crate::cryptography::{ECCurve, ECFieldElement};

/// Represents a (X,Y) coordinate pair for elliptic curve cryptography (ecc) structures.
#[derive(Clone,Debug)]
pub struct ECPoint {
    x: Option<ECFieldElement>,
    y: Option<ECFieldElement>,
    curve: ECCurve,
    compressed_point: Option<Vec<u8>>,
    uncompressed_point: Option<Vec<u8>>,
}

impl ECPoint {
    /// Indicates whether it is a point at infinity.
    pub fn is_infinity(&self) -> bool {
        self.x.is_none() && self.y.is_none()
    }

    pub fn size(&self) -> usize {
        if self.is_infinity() { 1 } else { 33 }
    }

    /// Initializes a new instance of the ECPoint struct with the secp256r1 curve.
    pub fn new() -> Self {
        Self::with_curve(ECCurve::SECP256R1)
    }

    pub fn with_curve(curve: ECCurve) -> Self {
        ECPoint {
            x: None,
            y: None,
            curve,
            compressed_point: None,
            uncompressed_point: None,
        }
    }

    pub fn with_coordinates(x: ECFieldElement, y: ECFieldElement, curve: ECCurve) -> Self {
        ECPoint {
            x: Some(x),
            y: Some(y),
            curve,
            compressed_point: None,
            uncompressed_point: None,
        }
    }
    pub fn encode_point(&self, compressed: bool) -> Vec<u8> {
        if self.is_infinity() {
            return vec![0x00];
        }

        let x = self.x.as_ref().unwrap();
        let y = self.y.as_ref().unwrap();

        if compressed {
            let mut buffer = vec![if y.is_even() { 0x02 } else { 0x03 }];
            buffer.extend_from_slice(&x.to_bytes());
            buffer
        } else {
            let mut buffer = vec![0x04];
            buffer.extend_from_slice(&x.to_bytes());
            buffer.extend_from_slice(&y.to_bytes());
            buffer
        }
    }

    pub fn decode_point(curve: &ECCurve, data: &[u8]) -> Result<Self, &'static str> {
        if data.is_empty() {
            return Err("Invalid point encoding");
        }

        match data[0] {
            0x00 => Ok(Self::with_curve(curve.clone())),
            0x02 | 0x03 => {
                if data.len() != 33 {
                    return Err("Invalid compressed point length");
                }
                let y_tilde = data[0] & 1;
                let x = ECFieldElement::from_bytes(curve, &data[1..]);
                let alpha = x.pow(3) + curve.a() * x + curve.b();
                let beta = alpha.sqrt();

                let y = if beta.is_even() == (y_tilde == 0) {
                    beta
                } else {
                    -beta
                };

                Ok(Self::with_coordinates(x, y, curve.clone()))
            }
            0x04 => {
                if data.len() != 65 {
                    return Err("Invalid uncompressed point length");
                }
                let x = ECFieldElement::from_bytes(curve, &data[1..33]);
                let y = ECFieldElement::from_bytes(curve, &data[33..]);
                Ok(Self::with_coordinates(x, y, curve.clone()))
            }
            _ => Err("Invalid point encoding"),
        }
    }

    pub fn add(&self, other: &Self) -> Result<Self, &'static str> {
        if self.curve != other.curve {
            return Err("Cannot add points on different curves");
        }

        if self.is_infinity() {
            return Ok(other.clone());
        }

        if other.is_infinity() {
            return Ok(self.clone());
        }

        let x1 = self.x.as_ref().unwrap();
        let y1 = self.y.as_ref().unwrap();
        let x2 = other.x.as_ref().unwrap();
        let y2 = other.y.as_ref().unwrap();

        if x1 == x2 {
            if y1 == y2 {
                return self.double();
            } else {
                return Ok(Self::with_curve(self.curve.clone()));
            }
        }

        let slope = (y2 - y1) / (x2 - x1);
        let x3 = slope.pow(2) - x1 - x2;
        let y3 = slope * (x1 - x3) - y1;

        Ok(Self::with_coordinates(x3, y3, self.curve.clone()))
    }

    pub fn double(&self) -> Result<Self, &'static str> {
        if self.is_infinity() {
            return Ok(self.clone());
        }

        let x = self.x.as_ref().unwrap();
        let y = self.y.as_ref().unwrap();

        if y.is_zero() {
            return Ok(Self::with_curve(self.curve.clone()));
        }

        let slope = (x.pow(2) * 3 + self.curve.a()) / (y * 2);
        let x3 = slope.pow(2) - x * 2;
        let y3 = slope * (x - x3) - y;

        Ok(Self::with_coordinates(x3, y3, self.curve.clone()))
    }

    pub fn multiply(&self, n: &BigInt) -> Result<Self, &'static str> {
        if n.is_zero() {
            return Ok(Self::with_curve(self.curve.clone()));
        }

        if self.is_infinity() {
            return Ok(self.clone());
        }

        let mut result = Self::with_curve(self.curve.clone());
        let mut addend = self.clone();
        let mut n = n.clone();

        while !n.is_zero() {
            if n.is_odd() {
                result = result.add(&addend)?;
            }
            addend = addend.double()?;
            n >>= 1;
        }

        Ok(result)
    }
}

impl PartialEq for ECPoint {
    fn eq(&self, other: &Self) -> bool {
        if std::ptr::eq(self, other) { return true; }
        if self.curve != other.curve { return false; }
        if self.is_infinity() && other.is_infinity() { return true; }
        if self.is_infinity() || other.is_infinity() { return false; }
        self.x == other.x && self.y == other.y
    }
}

impl Eq for ECPoint {}

impl PartialOrd for ECPoint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ECPoint {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.curve != other.curve {
            panic!("Invalid comparison for points with different curves");
        }
        if std::ptr::eq(self, other) { return Ordering::Equal; }
        match self.x.cmp(&other.x) {
            Ordering::Equal => self.y.cmp(&other.y),
            other => other,
        }
    }
}

impl fmt::Display for ECPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.encode_point(true)))
    }
}
