use std::str::FromStr;
use num_bigint::BigInt;
use crate::cryptography::{ECFieldElement, ECPoint};

#[derive(Clone,Debug)]
pub struct ECCurve {
    pub(crate) q: BigInt,
    a: ECFieldElement,
    b: ECFieldElement,
    n: BigInt,
    infinity: ECPoint,
    g: ECPoint,
    expected_ec_point_length: usize,
}

impl ECCurve {
    fn new(q: BigInt, a: BigInt, b: BigInt, n: BigInt, g: &[u8]) -> Self {
        let expected_ec_point_length = ((q.bits() as usize + 7) / 8) as usize;
        let a = ECFieldElement::new(a, &q);
        let b = ECFieldElement::new(b, &q);
        let infinity = ECPoint::new(None, None, &q);
        let g = ECPoint::decode_point(g, &q);

        ECCurve {
            q,
            a,
            b,
            n,
            infinity,
            g,
            expected_ec_point_length,
        }
    }

    pub const SECP256K1: ECCurve = ECCurve::new(
        BigInt::from_str("FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F").unwrap(),
        BigInt::from(0),
        BigInt::from(7),
        BigInt::from_str("FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141").unwrap(),
        &hex::decode("0479BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798483ADA7726A3C4655DA4FBFC0E1108A8FD17B448A68554199C47D08FFB10D4B8").unwrap(),
    );

    pub const SECP256R1: ECCurve = ECCurve::new(
        BigInt::from_str("FFFFFFFF00000001000000000000000000000000FFFFFFFFFFFFFFFFFFFFFFFF").unwrap(),
        BigInt::from_str("FFFFFFFF00000001000000000000000000000000FFFFFFFFFFFFFFFFFFFFFFFC").unwrap(),
        BigInt::from_str("5AC635D8AA3A93E7B3EBBD55769886BC651D06B0CC53B0F63BCE3C3E27D2604B").unwrap(),
        BigInt::from_str("FFFFFFFF00000000FFFFFFFFFFFFFFFFBCE6FAADA7179E84F3B9CAC2FC632551").unwrap(),
        &hex::decode("046B17D1F2E12C4247F8BCE6E563A440F277037D812DEB33A0F4A13945D898C2964FE342E2FE1A7F9B8EE7EB4A7C0F9E162BCE33576B315ECECBB6406837BF51F5").unwrap(),
    );
}
