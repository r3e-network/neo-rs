use alloc::rc::Rc;
use core::fmt::{Debug, Formatter};
use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};
use num_bigint::{BigInt, BigUint};
use num_traits::One;
use rand::Rng;
use crate::cryptography::ecc::ECCurve;
use crate::utility::BigIntExt;

#[derive(Clone)]
pub struct ECFieldElement {
    value: BigUint,
    pub(crate) curve: Rc<ECCurve>,
}

impl Debug for ECFieldElement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "ECFieldElement {{ value: {}, curve: {:?} }}", self.value, self.curve)
    }
}

impl ECFieldElement {
    fn new(value: BigUint, curve: Rc<ECCurve>) -> Self {
        ECFieldElement { value, curve }
    }
}

impl ECFieldElement {
    pub fn new(value: BigUint, curve: Rc<ECCurve>) -> Result<Self, &'static str> {
        if value >= curve.q {
            return Err("x value too large in field element");
        }
        Ok(Self { value, curve })
    }

    pub fn from_bytes(bytes: &[u8], curve: ECCurve) -> Result<Self, &'static str> {
        let value = BigInt::from_bytes_be(num_bigint::Sign::Plus, bytes);
        Ok(Self::new(value, Rc::new(curve)))
    }

    pub fn sqrt(&self) -> Option<ECFieldElement> {
        if self.curve.q.bit(1) {
            let z = ECFieldElement::new(
                self.value.modpow(&((&self.curve.q >> 2) + 1), &self.curve.q),
                self.curve.clone(),
            )
                .unwrap();
            if z.square().eq(self) {
                Some(z)
            } else {
                None
            }
        } else {
            let q_minus_one = &self.curve.q - 1;
            let legendre_exponent = &q_minus_one >> 1;
            if self.value.modpow(&legendre_exponent, &self.curve.q) != One::one() {
                return None;
            }
            let u = &q_minus_one >> 2;
            let k = (&u << 1) + 1;
            let q = &self.value;
            let four_q = (q << 2)%(&BigInt::one(), &self.curve.q);

            loop {
                let p = loop {
                    let p = BigInt::from(rand::thread_rng().gen::<u64>());
                    if p < self.curve.q
                        && p.pow(2).sub(&four_q)%(&legendre_exponent, &self.curve.q)
                        == q_minus_one
                    {
                        break p;
                    }
                };

                let result = Self::fast_lucas_sequence(&self.curve.q, &p, q, &k);
                let u = &result[0];
                let v = &result[1];

                if v.pow(2).modpow(&BigInt::one(), &self.curve.q) == four_q {
                    let mut v = v.clone();
                    if v.bit(0) {
                        v += &self.curve.q;
                    }
                    v >>= 1;
                    return ECFieldElement::new(v, self.curve.clone()).ok();
                }

                if u == &BigInt::one() || u == &q_minus_one {
                    continue;
                }

                return None;
            }
        }
    }

    pub fn square(&self) -> ECFieldElement {
        ECFieldElement::new(
            (&self.value * &self.value).modpow(&BigUint::from(1), &self.curve.q),
            self.curve.clone(),
        ).unwrap()
    }

    pub fn to_byte_array(&self) -> Vec<u8> {
        let mut data = self.value.to_bytes_be();
        if data.len() == 32 {
            data
        } else {
            let mut buffer = vec![0; 32];
            buffer[32 - data.len()..].copy_from_slice(&data);
            buffer
        }
    }

    fn fast_lucas_sequence(p: &BigInt, p_param: &BigInt, q: &BigInt, k: &BigInt) -> Vec<BigInt> {
        let n = k.get_bit_length();
        let s = k.trailing_zeros().unwrap();

        let mut uh = BigInt::from(1);
        let mut vl = BigInt::from(2);
        let mut vh = p_param.clone();
        let mut ql = BigInt::from(1);
        let mut qh = BigInt::from(1);

        for j in (s + 1..n).rev() {
            ql = (&ql * &qh).modpow(&BigInt::from(1), p);

            if k.bit(j as u64) {
                qh = (&ql * q).modpow(&BigInt::from(1), p);
                uh = (&uh * &vh).modpow(&BigInt::from(1), p);
                vl = (&vh * &vl - p_param * &ql).modpow(&BigInt::from(1), p);
                vh = (&vh * &vh - &qh * BigInt::from(2)).modpow(&BigInt::from(1), p);
            } else {
                qh = ql.clone();
                uh = (&uh * &vl - &ql).modpow(&BigInt::from(1), p);
                vh = (&vh * &vl - p_param * &ql).modpow(&BigInt::from(1), p);
                vl = (&vl * &vl - &ql * BigInt::from(2)).modpow(&BigInt::from(1), p);
            }
        }

        ql = (&ql * &qh).modpow(&BigInt::from(1), p);
        qh = (&ql * q).modpow(&BigInt::from(1), p);
        uh = (&uh * &vl - &ql).modpow(&BigInt::from(1), p);
        vl = (&vh * &vl - p_param * &ql).modpow(&BigInt::from(1), p);
        ql = (&ql * &qh).modpow(&BigInt::from(1), p);

        for _ in 1..=s {
            uh = (&uh * &vl).modpow(&BigInt::from(1), p);
            vl = (&vl * &vl - &ql * BigInt::from(2)).modpow(&BigInt::from(1), p);
            ql = (&ql * &ql).modpow(&BigInt::from(1), p);
        }

        vec![uh, vl]
    }
}

impl PartialEq for ECFieldElement {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.curve.q == other.curve.q
    }
}

impl Eq for ECFieldElement {}

impl PartialOrd for ECFieldElement {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ECFieldElement {
    fn cmp(&self, other: &Self) -> Ordering {
        if !self.curve.eq(&other.curve) {
            panic!("Invalid comparison for points with different curves");
        }
        self.value.cmp(&other.value)
    }
}

impl Neg for ECFieldElement {
    type Output = Self;

    fn neg(self) -> Self::Output {
        ECFieldElement::new((-&self.value).modpow(&BigInt::from(1), &self.curve.q), self.curve.clone()).unwrap()
    }
}

impl Add for ECFieldElement {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        ECFieldElement::new((&self.value + &rhs.value).modpow(&BigUint::from(1), &self.curve.q), self.curve.clone()).unwrap()
    }
}

impl Sub for ECFieldElement {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        ECFieldElement::new((&self.value - &rhs.value).modpow(&BigUint::from(1), &self.curve.q), self.curve.clone()).unwrap()
    }
}

impl Mul for ECFieldElement {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        ECFieldElement::new((&self.value * &rhs.value).modpow(&BigUint::from(1), &self.curve.q), self.curve.clone()).unwrap()
    }
}

impl Div for ECFieldElement {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        ECFieldElement::new(
            (&self.value * rhs.value.modpow(&(&self.curve.q - 2), &self.curve.q)).modpow(&BigUint::from(1), &self.curve.q),
            self.curve.clone()
        ).unwrap()
    }
}
