use std::any::Any;
use std::error::Error;
use std::fmt;
use num_bigint::BigInt;

use bls12_381::{G1Affine, G1Projective, G2Affine, G2Projective, Gt};
use crate::vm::stackitem::{Equatable, Interop};

// BlsPoint is a wrapper around bls12_381 point types that must be used as
// Interop values and implement Equatable trait.
#[derive(Clone)]
pub struct BlsPoint {
    point: Box<dyn Any>,
}

impl Equatable for BlsPoint {
    fn equals(&self, other: &dyn Equatable) -> bool {
        if let Some(other) = other.as_any().downcast_ref::<BlsPoint>() {
            self.equals_check_type(other).unwrap_or(false)
        } else {
            false
        }
    }
}

impl BlsPoint {
    fn equals_check_type(&self, other: &BlsPoint) -> Result<bool, Box<dyn Error>> {
        match (self.point.downcast_ref::<G1Affine>(), other.point.downcast_ref::<G1Affine>()) {
            (Some(x), Some(y)) => Ok(x == y),
            _ => match (self.point.downcast_ref::<G1Projective>(), other.point.downcast_ref::<G1Projective>()) {
                (Some(x), Some(y)) => Ok(x == y),
                _ => match (self.point.downcast_ref::<G2Affine>(), other.point.downcast_ref::<G2Affine>()) {
                    (Some(x), Some(y)) => Ok(x == y),
                    _ => match (self.point.downcast_ref::<G2Projective>(), other.point.downcast_ref::<G2Projective>()) {
                        (Some(x), Some(y)) => Ok(x == y),
                        _ => match (self.point.downcast_ref::<Gt>(), other.point.downcast_ref::<Gt>()) {
                            (Some(x), Some(y)) => Ok(x == y),
                            _ => Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, "Mismatched BLS12-381 point types"))),
                        },
                    },
                },
            },
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        if let Some(p) = self.point.downcast_ref::<G1Affine>() {
            p.to_compressed().to_vec()
        } else if let Some(p) = self.point.downcast_ref::<G1Projective>() {
            p.to_affine().to_compressed().to_vec()
        } else if let Some(p) = self.point.downcast_ref::<G2Affine>() {
            p.to_compressed().to_vec()
        } else if let Some(p) = self.point.downcast_ref::<G2Projective>() {
            p.to_affine().to_compressed().to_vec()
        } else if let Some(p) = self.point.downcast_ref::<Gt>() {
            p.to_bytes().to_vec()
        } else {
            panic!("Unknown BLS12-381 point type")
        }
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, Box<dyn Error>> {
        match buf.len() {
            48 => {
                let g1 = G1Affine::from_compressed(buf.try_into()?)?;
                Ok(BlsPoint { point: Box::new(g1) })
            },
            96 => {
                let g2 = G2Affine::from_compressed(buf.try_into()?)?;
                Ok(BlsPoint { point: Box::new(g2) })
            },
            576 => {
                let gt = Gt::from_bytes(buf.try_into()?)?;
                Ok(BlsPoint { point: Box::new(gt) })
            },
            _ => Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, "Invalid buffer length for BLS12-381 point"))),
        }
    }
}

pub fn bls_point_add(a: &BlsPoint, b: &BlsPoint) -> Result<BlsPoint, Box<dyn Error>> {
    match (a.point.downcast_ref::<G1Affine>(), b.point.downcast_ref::<G1Affine>()) {
        (Some(x), Some(y)) => {
            let res = G1Projective::from(x) + y;
            Ok(BlsPoint { point: Box::new(res) })
        },
        _ => match (a.point.downcast_ref::<G1Projective>(), b.point.downcast_ref::<G1Projective>()) {
            (Some(x), Some(y)) => {
                let res = x + y;
                Ok(BlsPoint { point: Box::new(res) })
            },
            _ => match (a.point.downcast_ref::<G2Affine>(), b.point.downcast_ref::<G2Affine>()) {
                (Some(x), Some(y)) => {
                    let res = G2Projective::from(x) + y;
                    Ok(BlsPoint { point: Box::new(res) })
                },
                _ => match (a.point.downcast_ref::<G2Projective>(), b.point.downcast_ref::<G2Projective>()) {
                    (Some(x), Some(y)) => {
                        let res = x + y;
                        Ok(BlsPoint { point: Box::new(res) })
                    },
                    _ => match (a.point.downcast_ref::<Gt>(), b.point.downcast_ref::<Gt>()) {
                        (Some(x), Some(y)) => {
                            let res = x * y;
                            Ok(BlsPoint { point: Box::new(res) })
                        },
                        _ => Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, "Inconsistent BLS12-381 point types for addition"))),
                    },
                },
            },
        },
    }
}

pub fn bls_point_mul(a: &BlsPoint, alpha: &BigInt) -> Result<BlsPoint, Box<dyn Error>> {
    let scalar = bls12_381::Scalar::from_be_bytes(alpha.to_bytes_be().1.try_into()?)?;
    
    match a.point.downcast_ref::<G1Affine>() {
        Some(x) => {
            let res = G1Projective::from(x) * scalar;
            Ok(BlsPoint { point: Box::new(res) })
        },
        None => match a.point.downcast_ref::<G1Projective>() {
            Some(x) => {
                let res = x * scalar;
                Ok(BlsPoint { point: Box::new(res) })
            },
            None => match a.point.downcast_ref::<G2Affine>() {
                Some(x) => {
                    let res = G2Projective::from(x) * scalar;
                    Ok(BlsPoint { point: Box::new(res) })
                },
                None => match a.point.downcast_ref::<G2Projective>() {
                    Some(x) => {
                        let res = x * scalar;
                        Ok(BlsPoint { point: Box::new(res) })
                    },
                    None => match a.point.downcast_ref::<Gt>() {
                        Some(x) => {
                            let res = x.pow(scalar);
                            Ok(BlsPoint { point: Box::new(res) })
                        },
                        None => Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, "Unexpected BLS12-381 point type for multiplication"))),
                    },
                },
            },
        },
    }
}

pub fn bls_point_pairing(a: &BlsPoint, b: &BlsPoint) -> Result<BlsPoint, Box<dyn Error>> {
    let g1 = match a.point.downcast_ref::<G1Affine>() {
        Some(x) => *x,
        None => match a.point.downcast_ref::<G1Projective>() {
            Some(x) => x.to_affine(),
            None => return Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, "Unexpected BLS12-381 point type for G1 in pairing"))),
        },
    };

    let g2 = match b.point.downcast_ref::<G2Affine>() {
        Some(x) => *x,
        None => match b.point.downcast_ref::<G2Projective>() {
            Some(x) => x.to_affine(),
            None => return Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, "Unexpected BLS12-381 point type for G2 in pairing"))),
        },
    };

    let gt = bls12_381::pairing(&g1, &g2);
    Ok(BlsPoint { point: Box::new(gt) })
}
