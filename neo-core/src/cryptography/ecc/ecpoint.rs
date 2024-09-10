use std::cmp::Ordering;
use std::rc::Rc;
use num_bigint::BigUint;
use std::collections::HashMap;
use std::sync::Mutex;
use futures::TryFutureExt;
use lazy_static::lazy_static;
use num_traits::ToPrimitive;
use crate::cryptography::{ECCurve, ECFieldElement};

#[derive(Clone, Debug)]
pub struct ECPoint {
    x: Option<ECFieldElement>,
    y: Option<ECFieldElement>,
    pub(crate) curve: Rc<ECCurve>,
    compressed_point: Option<Vec<u8>>,
    uncompressed_point: Option<Vec<u8>>,
}

lazy_static! {
    static ref POINT_CACHE_K1: Mutex<ECPointCache> = Mutex::new(ECPointCache::new(1000));
    static ref POINT_CACHE_R1: Mutex<ECPointCache> = Mutex::new(ECPointCache::new(1000));
}

impl ECPoint {
    pub fn new(x: Option<ECFieldElement>, y: Option<ECFieldElement>, curve: Rc<ECCurve>) -> Self {
        if (x.is_some() ^ y.is_some()) || curve.is_none() {
            panic!("Exactly one of the field elements is null");
        }
        ECPoint {
            x,
            y,
            curve,
            compressed_point: None,
            uncompressed_point: None,
        }
    }

    /// Creates a new ECPoint with the same coordinates but on a different curve.
    ///
    /// # Arguments
    ///
    /// * `new_curve` - The new curve to associate with the point.
    ///
    /// # Returns
    ///
    /// * `Result<ECPoint, &'static str>` - The new ECPoint on the specified curve, or an error if the operation is invalid.
    pub fn with_curve(&self, new_curve: Rc<ECCurve>) -> Result<ECPoint, &'static str> {
        if self.is_infinity() {
            return Ok(ECPoint::new(None, None, new_curve));
        }

        // Check if the point's coordinates are valid for the new curve
        if let (Some(x), Some(y)) = (&self.x, &self.y) {
            let new_x = ECFieldElement::new(x.value().clone(), Rc::clone(&new_curve));
            let new_y = ECFieldElement::new(y.value().clone(), Rc::clone(&new_curve));

            // Verify that the point satisfies the curve equation: y^2 = x^3 + ax + b
            let left = new_y.square();
            let right = &new_x.cube() + &(&new_curve.a * &new_x) + &new_curve.b;

            if left == right {
                Ok(ECPoint::new(Some(new_x), Some(new_y), new_curve))
            } else {
                Err("Point coordinates are not valid for the new curve")
            }
        } else {
            Err("Invalid point state")
        }
    }

    pub fn is_infinity(&self) -> bool {
        self.x.is_none() && self.y.is_none()
    }

    pub fn size(&self) -> usize {
        if self.is_infinity() { 1 } else { 33 }
    }

    pub fn decode_point(encoded: &[u8], curve: Rc<ECCurve>) -> Rc<Self> {
        match encoded[0] {
            0x02 | 0x03 => {
                if encoded.len() != (curve.expected_ec_point_length + 1) {
                    panic!("Incorrect length for compressed encoding");
                }
                Self::decompress_point(encoded, curve)
            },
            0x04 => {
                if encoded.len() != (2 * curve.expected_ec_point_length + 1) {
                    panic!("Incorrect length for uncompressed/hybrid encoding");
                }
                let x1 = BigUint::from_bytes_be(&encoded[1..1 + curve.expected_ec_point_length]);
                let y1 = BigUint::from_bytes_be(&encoded[1 + curve.expected_ec_point_length..]);
                let point = Rc::new(ECPoint {
                    x: Some(ECFieldElement::new(x1, Rc::clone(&curve)).unwrap()),
                    y: Some(ECFieldElement::new(y1, Rc::clone(&curve)).unwrap()),
                    curve: Rc::clone(&curve),
                    compressed_point: None,
                    uncompressed_point: Some(encoded.to_vec()),
                });
                point
            },
            _ => panic!("Invalid point encoding"),
        }
    }

    fn decompress_point(encoded: &[u8], curve: Rc<ECCurve>) -> Rc<Self> {
        let point_cache = if Rc::ptr_eq(&curve, &ECCurve::secp256r1()) {
            &POINT_CACHE_R1
        } else if Rc::ptr_eq(&curve, &ECCurve::secp256r1()) {
            &POINT_CACHE_R1
        } else {
            panic!("Invalid curve");
        };

        let compressed_point = encoded.to_vec();
        if let Some(p) = point_cache.lock().unwrap().try_get(&compressed_point) {
            return p;
        }

        let y_tilde = encoded[0] & 1;
        let x1 = BigUint::from_bytes_be(&encoded[1..]);
        let p = Self::decompress_point_internal(y_tilde, x1, Rc::clone(&curve));
        let point = Rc::new(p);
        point_cache.lock().unwrap().add(Rc::clone(&point));
        point
    }

    fn decompress_point_internal(y_tilde: u8, x1: BigUint, curve: Rc<ECCurve>) -> Self {
        let x = ECFieldElement::new(x1, Rc::clone(&curve));
        let alpha = &x * (&x.square() + &curve.a) + &curve.b;
        let beta = alpha.sqrt();

        if beta.is_none() {
            panic!("Invalid point compression");
        }

        let mut beta = beta.unwrap();
        let beta_value = beta.value();
        let bit0 = beta_value.bit(0) as u8;

        if bit0 != y_tilde {
            beta = ECFieldElement::new(&curve.q - beta_value, Rc::clone(&curve));
        }

        ECPoint {
            x: Some(x),
            y: Some(beta),
            curve,
            compressed_point: None,
            uncompressed_point: None,
        }
    }

    pub fn encode_point(&mut self, compressed: bool) -> Vec<u8> {
        if self.is_infinity() {
            return vec![0];
        }

        if compressed {
            if let Some(ref point) = self.compressed_point {
                return point.clone();
            }
            let mut data = vec![0; 33];
            let x_bytes = self.x.as_ref().unwrap().value().to_bytes_be();
            data[33 - x_bytes.len()..].copy_from_slice(&x_bytes);
            data[0] = if self.y.as_ref().unwrap().value().bit(0) { 0x03 } else { 0x02 };
            self.compressed_point = Some(data.clone());
            data
        } else {
            if let Some(ref point) = self.uncompressed_point {
                return point.clone();
            }
            let mut data = vec![0; 65];
            let x_bytes = self.x.as_ref().unwrap().value().to_bytes_be();
            let y_bytes = self.y.as_ref().unwrap().value().to_bytes_be();
            data[33 - x_bytes.len()..33].copy_from_slice(&x_bytes);
            data[65 - y_bytes.len()..].copy_from_slice(&y_bytes);
            data[0] = 0x04;
            self.uncompressed_point = Some(data.clone());
            data
        }
    }

    pub fn negate(&self) -> Self {
        ECPoint {
            x: self.x.clone(),
            y: self.y.as_ref().map(|y| -y.clone()),
            curve: Rc::clone(&self.curve),
            compressed_point: None,
            uncompressed_point: None,
        }
    }
}

impl PartialEq for ECPoint {
    fn eq(&self, other: &Self) -> bool {
        if !Rc::ptr_eq(&self.curve, &other.curve) {
            return false;
        }
        if self.is_infinity() && other.is_infinity() {
            return true;
        }
        if self.is_infinity() || other.is_infinity() {
            return false;
        }
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
        if !Rc::ptr_eq(&self.curve, &other.curve) {
            panic!("Invalid comparison for points with different curves");
        }
        match self.x.cmp(&other.x) {
            Ordering::Equal => self.y.cmp(&other.y),
            other => other,
        }
    }
}

impl ECPoint {
pub fn multiply(&self, k: &BigUint) -> Self {
    // floor(log2(k))
    let m = k.bits() as usize;

    // width of the Window NAF
    let width: i8;

    // Required length of precomputing array
    let req_pre_comp_len: usize;

    // Determine optimal width and corresponding length of precomputing array
    // array based on literature values
    if m < 13 {
        width = 2;
        req_pre_comp_len = 1;
    } else if m < 41 {
        width = 3;
        req_pre_comp_len = 2;
    } else if m < 121 {
        width = 4;
        req_pre_comp_len = 4;
    } else if m < 337 {
        width = 5;
        req_pre_comp_len = 8;
    } else if m < 897 {
        width = 6;
        req_pre_comp_len = 16;
    } else if m < 2305 {
        width = 7;
        req_pre_comp_len = 32;
    } else {
        width = 8;
        req_pre_comp_len = 127;
    }

    // The length of the precomputing array
    let mut pre_comp_len = 1;

    let mut pre_comp = vec![self.clone()];
    let twice_p = self.twice();

    if pre_comp_len < req_pre_comp_len {
        // Precomputing array must be made bigger, copy existing preComp
        // array into the larger new preComp array
        let old_pre_comp = pre_comp;
        pre_comp = vec![ECPoint::new(None, None, Rc::clone(&self.curve)); req_pre_comp_len];
        pre_comp[0] = old_pre_comp[0].clone();

        for i in pre_comp_len..req_pre_comp_len {
            // Compute the new ECPoints for the precomputing array.
            // The values 1, 3, 5, ..., 2^(width-1)-1 times p are computed
            pre_comp[i] = &twice_p + &pre_comp[i - 1];
        }
    }

    // Compute the Window NAF of the desired width
    let wnaf = Self::window_naf(width, k);
    let l = wnaf.len();

    // Apply the Window NAF to p using the precomputed ECPoint values.
    let mut q = ECPoint::new(None, None, Rc::clone(&self.curve));
    for i in (0..l).rev() {
        q = q.twice();

        if wnaf[i] != 0 {
            if wnaf[i] > 0 {
                q = &q + &pre_comp[((wnaf[i] - 1) / 2) as usize];
            } else {
                // wnaf[i] < 0
                q = &q - &pre_comp[((-wnaf[i] - 1) / 2) as usize];
            }
        }
    }

    q
}

pub fn twice(&self) -> Self {
    if self.is_infinity() {
        return self.clone();
    }
    if self.y.as_ref().unwrap().value() == &BigUint::from(0u32) {
        return ECPoint::new(None, None, Rc::clone(&self.curve));
    }
    let two = ECFieldElement::new(BigUint::from(2u32), Rc::clone(&self.curve));
    let three = ECFieldElement::new(BigUint::from(3u32), Rc::clone(&self.curve));
    let x = self.x.as_ref().unwrap();
    let y = self.y.as_ref().unwrap();
    let gamma = (x.square() * &three + &self.curve.a) / (y * &two);
    let x3 = &gamma.square() - &(x * &two);
    let y3 = &gamma * (x - &x3) - y;
    ECPoint::new(Some(x3), Some(y3), Rc::clone(&self.curve))
}

fn window_naf(width: i8, k: &BigUint) -> Vec<i8> {
    let mut wnaf = vec![0i8; k.bits() as usize + 1];
    let pow2w_b = BigUint::from(1u32) << width;
    let mut i = 0;
    let mut length = 0;
    let mut k = k.clone();
    while k > BigUint::from(0u32) {
        if k.bit(0) {
            let mut remainder = &k % &pow2w_b.clone();
            if remainder.bit(width as u64 - 1) {
                wnaf[i] = -((pow2w_b - remainder).to_i8().unwrap());
            } else {
                wnaf[i] = remainder.to_i8().unwrap();
            }
            k -= BigUint::from(wnaf[i].abs());
            length = i;
        } else {
            wnaf[i] = 0;
        }
        k >>= 1;
        i += 1;
    }
    wnaf[..length + 1].to_vec()
}

pub fn negate(&self) -> Self {
    if self.is_infinity() {
        return self.clone();
    }
    ECPoint {
        x: self.x.clone(),
        y: self.y.as_ref().map(|y| -y.clone()),
        curve: Rc::clone(&self.curve),
        compressed_point: None,
        uncompressed_point: None,
    }
}

pub fn multiply_bytes(&self, n: &[u8]) -> Result<Self, &'static str> {
    if n.len() != 32 {
        return Err("Invalid byte array length for multiplication");
    }
    if self.is_infinity() {
        return Ok(self.clone());
    }
    let k = BigUint::from_bytes_be(n);
    if k == BigUint::from(0u32) {
        return Ok(ECPoint::new(None, None, Rc::clone(&self.curve)));
    }
    Ok(self.multiply(&k))
}
}

// Implement negation
impl std::ops::Neg for &ECPoint {
    type Output = ECPoint;

    fn neg(self) -> Self::Output {
        self.negate()
    }
}

// Implement addition
impl std::ops::Add for &ECPoint {
    type Output = ECPoint;

    fn add(self, other: &ECPoint) -> ECPoint {
        if self.is_infinity() {
            return other.clone();
        }
        if other.is_infinity() {
            return self.clone();
        }
        if self.x == other.x {
            if self.y == other.y {
                return self.twice();
            }
            return ECPoint::new(None, None, Rc::clone(&self.curve));
        }
        let gamma = (other.y.as_ref().unwrap() - self.y.as_ref().unwrap()) / (other.x.as_ref().unwrap() - self.x.as_ref().unwrap());
        let x3 = &gamma.square() - self.x.as_ref().unwrap() - other.x.as_ref().unwrap();
        let y3 = &gamma * (self.x.as_ref().unwrap() - &x3) - self.y.as_ref().unwrap();
        ECPoint::new(Some(x3), Some(y3), Rc::clone(&self.curve))
    }
}

// Implement subtraction
impl std::ops::Sub for &ECPoint {
    type Output = ECPoint;

    fn sub(self, other: &ECPoint) -> ECPoint {
        if other.is_infinity() {
            return self.clone();
        }
        self + &(-other)
    }
}

// Implement multiplication by a byte array (scalar multiplication)
impl std::ops::Mul<&[u8]> for &ECPoint {
    type Output = Result<ECPoint, &'static str>;

    fn mul(self, n: &[u8]) -> Self::Output {
        self.multiply_bytes(n)
    }
}

// Implement Display for ECPoint
impl std::fmt::Display for ECPoint {
    fn fmt(&mut self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.encode_point(true)))
    }
}

// Implement FromStr for ECPoint
impl std::str::FromStr for ECPoint {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s).map_err(|_| "Invalid hex string")?;
        ECPoint::decode_point(&bytes, Rc::clone(&ECCurve::secp256r1()))
            .map_err(|_| "Invalid point encoding")
    }
}

// Helper trait for BigUint
trait BigUintExt {
    fn bit(&self, i: u64) -> bool;
    fn to_i8(&self) -> Option<i8>;
}

impl BigUintExt for BigUint {
    fn bit(&self, i: u64) -> bool {
        (self >> i) & BigUint::from(1u32) == BigUint::from(1u32)
    }

    fn to_i8(&self) -> Option<i8> {
        if *self <= BigUint::from(i8::MAX as u8) {
            Some(self.to_u8().unwrap() as i8)
        } else {
            None
        }
    }
}

// Implement Add, Sub traits for ECPoint
impl std::ops::Add for &ECPoint {
    type Output = ECPoint;

    fn add(self, other: &ECPoint) -> ECPoint {
        if self.is_infinity() {
            return other.clone();
        }
        if other.is_infinity() {
            return self.clone();
        }
        if self.x == other.x {
            if self.y == other.y {
                return self.twice();
            }
            return ECPoint::new(None, None, Rc::clone(&self.curve));
        }
        let gamma = (other.y.as_ref().unwrap() - self.y.as_ref().unwrap()) / (other.x.as_ref().unwrap() - self.x.as_ref().unwrap());
        let x3 = &gamma.square() - self.x.as_ref().unwrap() - other.x.as_ref().unwrap();
        let y3 = &gamma * (self.x.as_ref().unwrap() - &x3) - self.y.as_ref().unwrap();
        ECPoint::new(Some(x3), Some(y3), Rc::clone(&self.curve))
    }
}

impl std::ops::Sub for &ECPoint {
    type Output = ECPoint;

    fn sub(self, other: &ECPoint) -> ECPoint {
        if other.is_infinity() {
            return self.clone();
        }
        self + &other.negate()
    }
}

impl std::ops::Add for &ECPoint {
    type Output = ECPoint;

    fn add(self, other: &ECPoint) -> ECPoint {
        if self.is_infinity() {
            return other.clone();
        }
        if other.is_infinity() {
            return self.clone();
        }
        if self.x == other.x {
            if self.y == other.y {
                return self.twice();
            }
            return ECPoint::new(None, None, Rc::clone(&self.curve));
        }
        let gamma = (other.y.as_ref().unwrap() - self.y.as_ref().unwrap()) / (other.x.as_ref().unwrap() - self.x.as_ref().unwrap());
        let x3 = &gamma.square() - self.x.as_ref().unwrap() - other.x.as_ref().unwrap();
        let y3 = &gamma * (self.x.as_ref().unwrap() - &x3) - self.y.as_ref().unwrap();
        ECPoint::new(Some(x3), Some(y3), Rc::clone(&self.curve))
    }
}

// ... (Implement other operations like Sub, Mul, etc.)