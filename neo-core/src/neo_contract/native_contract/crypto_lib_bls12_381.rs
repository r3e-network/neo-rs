
use neo::prelude::*;
use neo::sys::InteropInterface;
use neo::types::{G1Affine, G1Projective, G2Affine, G2Projective, Gt, Scalar};
use neo::vm_api::Bls12;
use neo_proc_macros::contract_method;

pub struct CryptoLib;

impl CryptoLib {
    /// Serialize a bls12381 point.
    #[contract_method(cpu_fee = 1 << 19)]
    pub fn bls12381_serialize(g: &InteropInterface) -> Vec<u8> {
        match g.get_interface::<Box<dyn Any>>().downcast_ref() {
            Some(p) if p.is::<G1Affine>() => p.to_compressed().to_vec(),
            Some(p) if p.is::<G1Projective>() => G1Affine::from(*p).to_compressed().to_vec(),
            Some(p) if p.is::<G2Affine>() => p.to_compressed().to_vec(),
            Some(p) if p.is::<G2Projective>() => G2Affine::from(*p).to_compressed().to_vec(),
            Some(p) if p.is::<Gt>() => p.to_bytes().to_vec(),
            _ => panic!("Bls12381 operation fault, type:format, error:type mismatch"),
        }
    }

    /// Deserialize a bls12381 point.
    #[contract_method(cpu_fee = 1 << 19)]
    pub fn bls12381_deserialize(data: &[u8]) -> InteropInterface {
        match data.len() {
            48 => InteropInterface::new(G1Affine::from_compressed(data).unwrap()),
            96 => InteropInterface::new(G2Affine::from_compressed(data).unwrap()),
            576 => InteropInterface::new(Gt::from_bytes(data).unwrap()),
            _ => panic!("Bls12381 operation fault, type:format, error:valid point length"),
        }
    }

    /// Determines whether the specified points are equal.
    #[contract_method(cpu_fee = 1 << 5)]
    pub fn bls12381_equal(x: &InteropInterface, y: &InteropInterface) -> bool {
        match (x.get_interface::<Box<dyn Any>>().downcast_ref(), y.get_interface::<Box<dyn Any>>().downcast_ref()) {
            (Some(p1), Some(p2)) if p1.is::<G1Affine>() && p2.is::<G1Affine>() => p1 == p2,
            (Some(p1), Some(p2)) if p1.is::<G1Projective>() && p2.is::<G1Projective>() => p1 == p2,
            (Some(p1), Some(p2)) if p1.is::<G2Affine>() && p2.is::<G2Affine>() => p1 == p2,
            (Some(p1), Some(p2)) if p1.is::<G2Projective>() && p2.is::<G2Projective>() => p1 == p2,
            (Some(p1), Some(p2)) if p1.is::<Gt>() && p2.is::<Gt>() => p1 == p2,
            _ => panic!("Bls12381 operation fault, type:format, error:type mismatch"),
        }
    }

    /// Add operation of two points.
    #[contract_method(cpu_fee = 1 << 19)]
    pub fn bls12381_add(x: &InteropInterface, y: &InteropInterface) -> InteropInterface {
        match (x.get_interface::<Box<dyn Any>>().downcast_ref(), y.get_interface::<Box<dyn Any>>().downcast_ref()) {
            (Some(p1), Some(p2)) if p1.is::<G1Affine>() && p2.is::<G1Affine>() => 
                InteropInterface::new(G1Projective::from(*p1) + *p2),
            (Some(p1), Some(p2)) if p1.is::<G1Affine>() && p2.is::<G1Projective>() => 
                InteropInterface::new(*p1 + *p2),
            (Some(p1), Some(p2)) if p1.is::<G1Projective>() && p2.is::<G1Affine>() => 
                InteropInterface::new(*p1 + *p2),
            (Some(p1), Some(p2)) if p1.is::<G1Projective>() && p2.is::<G1Projective>() => 
                InteropInterface::new(*p1 + *p2),
            (Some(p1), Some(p2)) if p1.is::<G2Affine>() && p2.is::<G2Affine>() => 
                InteropInterface::new(G2Projective::from(*p1) + *p2),
            (Some(p1), Some(p2)) if p1.is::<G2Affine>() && p2.is::<G2Projective>() => 
                InteropInterface::new(*p1 + *p2),
            (Some(p1), Some(p2)) if p1.is::<G2Projective>() && p2.is::<G2Affine>() => 
                InteropInterface::new(*p1 + *p2),
            (Some(p1), Some(p2)) if p1.is::<G2Projective>() && p2.is::<G2Projective>() => 
                InteropInterface::new(*p1 + *p2),
            (Some(p1), Some(p2)) if p1.is::<Gt>() && p2.is::<Gt>() => 
                InteropInterface::new(*p1 + *p2),
            _ => panic!("Bls12381 operation fault, type:format, error:type mismatch"),
        }
    }

    /// Mul operation of gt point and multiplier
    #[contract_method(cpu_fee = 1 << 21)]
    pub fn bls12381_mul(x: &InteropInterface, mul: &[u8], neg: bool) -> InteropInterface {
        let scalar = if neg { -Scalar::from_bytes(mul) } else { Scalar::from_bytes(mul) };
        match x.get_interface::<Box<dyn Any>>().downcast_ref() {
            Some(p) if p.is::<G1Affine>() => InteropInterface::new(*p * scalar),
            Some(p) if p.is::<G1Projective>() => InteropInterface::new(*p * scalar),
            Some(p) if p.is::<G2Affine>() => InteropInterface::new(*p * scalar),
            Some(p) if p.is::<G2Projective>() => InteropInterface::new(*p * scalar),
            Some(p) if p.is::<Gt>() => InteropInterface::new(*p * scalar),
            _ => panic!("Bls12381 operation fault, type:format, error:type mismatch"),
        }
    }

    /// Pairing operation of g1 and g2
    #[contract_method(cpu_fee = 1 << 23)]
    pub fn bls12381_pairing(g1: &InteropInterface, g2: &InteropInterface) -> InteropInterface {
        let g1a = match g1.get_interface::<Box<dyn Any>>().downcast_ref() {
            Some(g) if g.is::<G1Affine>() => *g,
            Some(g) if g.is::<G1Projective>() => G1Affine::from(*g),
            _ => panic!("Bls12381 operation fault, type:format, error:type mismatch"),
        };
        let g2a = match g2.get_interface::<Box<dyn Any>>().downcast_ref() {
            Some(g) if g.is::<G2Affine>() => *g,
            Some(g) if g.is::<G2Projective>() => G2Affine::from(*g),
            _ => panic!("Bls12381 operation fault, type:format, error:type mismatch"),
        };
        InteropInterface::new(Bls12::pairing(&g1a, &g2a))
    }
}
