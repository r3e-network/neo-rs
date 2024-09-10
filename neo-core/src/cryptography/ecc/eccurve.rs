use num_bigint::BigUint;
use hex;
use std::rc::Rc;
use crate::cryptography::{ECFieldElement, ECPoint};

/// Represents an elliptic curve.
#[derive(Debug)]
pub struct ECCurve {
    pub q: BigUint,
    pub a: ECFieldElement,
    pub b: ECFieldElement,
    pub n: BigUint,
    /// The point at infinity.
    pub infinity: ECPoint,
    /// The generator, or base point, for operations on the curve.
    pub g: ECPoint,
    pub expected_ec_point_length: usize,
}

impl Default for ECCurve {
    fn default() -> Self {
        let secp256r1 = Self::secp256r1();
        ECCurve {
            q: secp256r1.q.clone(),
            a: secp256r1.a.clone(),
            b: secp256r1.b.clone(),
            n: secp256r1.n.clone(),
            infinity: secp256r1.infinity.clone(),
            g: secp256r1.g.clone(),
            expected_ec_point_length: secp256r1.expected_ec_point_length,
        }
    }
}


impl ECCurve {
    fn new(q: BigUint, a: BigUint, b: BigUint, n: BigUint, g: &[u8]) -> Rc<Self> {
        let curve = Rc::new(ECCurve {
            q: q.clone(),
            a: ECFieldElement::new(a.clone(), Rc::new(ECCurve::default())).unwrap(),  // Temporary Rc, will be replaced
            b: ECFieldElement::new(b.clone(), Rc::new(ECCurve::default())).unwrap(),  // Temporary Rc, will be replaced
            n,
            infinity: ECPoint::with_curve(Rc::new(ECCurve::default())), // Temporary Rc, will be replaced
            g: ECPoint::with_curve(Rc::new(ECCurve::default()) ),        // Temporary Rc, will be replaced
            expected_ec_point_length: ((q.bits() as usize + 7) / 8) as usize,
        });

        // Replace the temporary Rc with the actual curve reference
        Rc::get_mut(&mut curve.a.curve).unwrap().replace(Rc::clone(&curve));
        Rc::get_mut(&mut curve.b.curve).unwrap().replace(Rc::clone(&curve));
        Rc::get_mut(&mut curve.infinity.curve).unwrap().replace(Rc::clone(&curve));
        
        // Decode the generator point
        let g_point = ECPoint::decode_point(g, Rc::clone(&curve));
        Rc::get_mut(&mut curve.g).unwrap().replace(g_point);

        curve
    }

    /// Represents a secp256k1 named curve.
    pub fn secp256k1() -> Rc<Self> {
        let q = BigUint::parse_bytes(b"FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F", 16).unwrap();
        let a = BigUint::from(0u32);
        let b = BigUint::from(7u32);
        let n = BigUint::parse_bytes(b"FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141", 16).unwrap();
        let g = hex::decode("0479BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798483ADA7726A3C4655DA4FBFC0E1108A8FD17B448A68554199C47D08FFB10D4B8").unwrap();
        
        Self::new(q, a, b, n, &g)
    }

    /// Represents a secp256r1 named curve.
    pub fn secp256r1() -> Rc<Self> {
        let q = BigUint::parse_bytes(b"FFFFFFFF00000001000000000000000000000000FFFFFFFFFFFFFFFFFFFFFFFF", 16).unwrap();
        let a = BigUint::parse_bytes(b"FFFFFFFF00000001000000000000000000000000FFFFFFFFFFFFFFFFFFFFFFFC", 16).unwrap();
        let b = BigUint::parse_bytes(b"5AC635D8AA3A93E7B3EBBD55769886BC651D06B0CC53B0F63BCE3C3E27D2604B", 16).unwrap();
        let n = BigUint::parse_bytes(b"FFFFFFFF00000000FFFFFFFFFFFFFFFFBCE6FAADA7179E84F3B9CAC2FC632551", 16).unwrap();
        let g = hex::decode("046B17D1F2E12C4247F8BCE6E563A440F277037D812DEB33A0F4A13945D898C2964FE342E2FE1A7F9B8EE7EB4A7C0F9E162BCE33576B315ECECBB6406837BF51F5").unwrap();
        
        Self::new(q, a, b, n, &g)
    }
}
