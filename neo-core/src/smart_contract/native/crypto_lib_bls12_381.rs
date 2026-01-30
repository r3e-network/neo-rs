use crate::{Error, Result};
// Removed neo_bls12_381 dependency - using blst crate directly
// BLS12-381 functionality is now available through crate::neo_core::crypto_utils::Bls12381Crypto

enum DecodedValue {
    G1(G1Projective),
    G2(G2Projective),
    Gt(Gt),
}

impl DecodedValue {
    fn to_canonical_bytes(&self) -> Vec<u8> {
        match self {
            DecodedValue::G1(point) => point.to_affine().to_compressed().to_vec(),
            DecodedValue::G2(point) => point.to_affine().to_compressed().to_vec(),
            DecodedValue::Gt(value) => value.to_bytes().to_vec(),
        }
    }
}

/// Serializes a BLS object (G1/G2/Gt) into its canonical byte representation.
pub fn serialize(data: &[u8]) -> Result<Vec<u8>> {
    let decoded = decode_value(data)?;
    Ok(decoded.to_canonical_bytes())
}

/// Deserializes and validates a BLS object, returning its canonical bytes.
pub fn deserialize(data: &[u8]) -> Result<Vec<u8>> {
    serialize(data)
}

/// Determines whether two BLS objects are equal.
pub fn equals(lhs: &[u8], rhs: &[u8]) -> Result<bool> {
    let left = decode_value(lhs)?;
    let right = decode_value(rhs)?;

    match (left, right) {
        (DecodedValue::G1(a), DecodedValue::G1(b)) => Ok(G1Affine::from(a) == G1Affine::from(b)),
        (DecodedValue::G2(a), DecodedValue::G2(b)) => Ok(G2Affine::from(a) == G2Affine::from(b)),
        (DecodedValue::Gt(a), DecodedValue::Gt(b)) => Ok(a == b),
        _ => Err(type_mismatch_error()),
    }
}

/// Adds two BLS objects of the same group.
pub fn add(lhs: &[u8], rhs: &[u8]) -> Result<Vec<u8>> {
    let left = decode_value(lhs)?;
    let right = decode_value(rhs)?;

    let result = match (left, right) {
        (DecodedValue::G1(a), DecodedValue::G1(b)) => DecodedValue::G1(a + b),
        (DecodedValue::G2(a), DecodedValue::G2(b)) => DecodedValue::G2(a + b),
        (DecodedValue::Gt(a), DecodedValue::Gt(b)) => DecodedValue::Gt(a + b),
        _ => return Err(type_mismatch_error()),
    };

    Ok(result.to_canonical_bytes())
}

/// Multiplies a BLS object by a scalar (optionally negating the scalar).
pub fn mul(point: &[u8], scalar: &[u8], neg: bool) -> Result<Vec<u8>> {
    let value = decode_value(point)?;
    let mut scalar = parse_scalar(scalar)?;

    if neg {
        scalar = -scalar;
    }

    let result = match value {
        DecodedValue::G1(p) => DecodedValue::G1(p * scalar),
        DecodedValue::G2(p) => DecodedValue::G2(p.mul_scalar(&scalar)),
        DecodedValue::Gt(gt) => DecodedValue::Gt(gt * scalar),
    };


    Ok(result.to_canonical_bytes())
}

/// Computes a pairing between a G1 and a G2 point, returning a GT element.
pub fn pairing(g1_bytes: &[u8], g2_bytes: &[u8]) -> Result<Vec<u8>> {
    let g1 = decode_value(g1_bytes)?;
    let g2 = decode_value(g2_bytes)?;

    let g1_affine = match g1 {
        DecodedValue::G1(p) => G1Affine::from(p),
        _ => {
            return Err(Error::native_contract(
                "bls12381Pairing requires a G1 point as the first argument".to_string(),
            ))
        }
    };

    let g2_affine = match g2 {
        DecodedValue::G2(p) => G2Affine::from(p),
        _ => {
            return Err(Error::native_contract(
                "bls12381Pairing requires a G2 point as the second argument".to_string(),
            ))
        }
    };

    let gt = bls_pairing(&g1_affine, &g2_affine);
    Ok(gt.to_bytes().to_vec())
}

fn decode_value(data: &[u8]) -> Result<DecodedValue> {
    match data.len() {
        G1_COMPRESSED_LEN => parse_g1_compressed(data),
        G1_UNCOMPRESSED_LEN => {
            parse_g1_uncompressed(data).or_else(|_| parse_g2_compressed(data))
        }
        G2_COMPRESSED_LEN => parse_g2_compressed(data),
        G2_UNCOMPRESSED_LEN => parse_g2_uncompressed(data),
        GT_SIZE => parse_gt(data),
        _ => Err(Error::native_contract(
            "Unsupported BLS12-381 encoding length".to_string(),
        )),
    }
}

fn parse_g1_compressed(data: &[u8]) -> Result<DecodedValue> {
    let point = G1Affine::from_compressed(data).map_err(map_bls_error)?;
    Ok(DecodedValue::G1(G1Projective::from(point)))
}

fn parse_g1_uncompressed(data: &[u8]) -> Result<DecodedValue> {
    let point = G1Affine::from_uncompressed(data).map_err(map_bls_error)?;
    Ok(DecodedValue::G1(G1Projective::from(point)))
}

fn parse_g2_compressed(data: &[u8]) -> Result<DecodedValue> {
    let point = G2Affine::from_compressed(data).map_err(map_bls_error)?;
    Ok(DecodedValue::G2(G2Projective::from(point)))
}

fn parse_g2_uncompressed(data: &[u8]) -> Result<DecodedValue> {
    let point = G2Affine::from_uncompressed(data).map_err(map_bls_error)?;
    Ok(DecodedValue::G2(G2Projective::from(point)))
}

fn parse_gt(data: &[u8]) -> Result<DecodedValue> {
    let gt = Gt::from_bytes(data).map_err(map_bls_error)?;
    Ok(DecodedValue::Gt(gt))
}

fn parse_scalar(bytes: &[u8]) -> Result<Scalar> {
    Scalar::from_bytes(bytes).map_err(map_bls_error)
}

fn map_bls_error(err: BlsError) -> Error {
    Error::native_contract(err.to_string())
}

fn type_mismatch_error() -> Error {
    Error::native_contract("BLS12-381 type mismatch")
}
