/// Module crypto provides an interface to CryptoLib native contract.
/// It implements some cryptographic functions.

use crate::interop;
use crate::interop::contract;
use crate::interop::neogointernal;

/// Hash represents CryptoLib contract hash.
const HASH: &str = "\x1b\xf5\x75\xab\x11\x89\x68\x84\x13\x61\x0a\x35\xa1\x28\x86\xcd\xe0\xb6\x6c\x72";

/// NamedCurveHash represents a pair of named elliptic curve and hash function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamedCurveHash(u8);

/// Various pairs of named elliptic curves and hash functions.
impl NamedCurveHash {
    pub const SECP256K1_SHA256: NamedCurveHash = NamedCurveHash(22);
    pub const SECP256R1_SHA256: NamedCurveHash = NamedCurveHash(23);
    pub const SECP256K1_KECCAK256: NamedCurveHash = NamedCurveHash(122);
    pub const SECP256R1_KECCAK256: NamedCurveHash = NamedCurveHash(123);
}

/// Sha256 calls `sha256` method of native CryptoLib contract and computes SHA256 hash of b.
pub fn sha256(b: &[u8]) -> interop::Hash256 {
    neogointernal::call_with_token(HASH, "sha256", contract::CallFlag::NONE_FLAG, b).unwrap()
}

/// Ripemd160 calls `ripemd160` method of native CryptoLib contract and computes RIPEMD160 hash of b.
pub fn ripemd160(b: &[u8]) -> interop::Hash160 {
    neogointernal::call_with_token(HASH, "ripemd160", contract::CallFlag::NONE_FLAG, b).unwrap()
}

/// Murmur32 calls `murmur32` method of native CryptoLib contract and computes Murmur32 hash of b
/// using the given seed.
pub fn murmur32(b: &[u8], seed: i32) -> Vec<u8> {
    neogointernal::call_with_token(HASH, "murmur32", contract::CallFlag::NONE_FLAG, b, seed).unwrap()
}

/// VerifyWithECDsa calls `verifyWithECDsa` method of native CryptoLib contract and checks that sig is
/// a correct msg's signature for the given pub (serialized public key on the given curve).
pub fn verify_with_ecdsa(msg: &[u8], pub: interop::PublicKey, sig: interop::Signature, curve_hash: NamedCurveHash) -> bool {
    neogointernal::call_with_token(HASH, "verifyWithECDsa", contract::CallFlag::NONE_FLAG, msg, pub, sig, curve_hash).unwrap()
}

/// Bls12381Point represents BLS12-381 curve point (G1 or G2 in the Affine or
/// Jacobian form or GT). Bls12381Point structure is needed for the operations
/// with the curve's points (serialization, addition, multiplication, pairing and
/// equality checks). It's an opaque type that can only be created properly by
/// Bls12381Deserialize, Bls12381Add, Bls12381Mul or Bls12381Pairing. The only
/// way to expose the Bls12381Point out of the runtime to the outside world is by
/// serializing it with Bls12381Serialize method call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bls12381Point;

/// Bls12381Serialize calls `bls12381Serialize` method of native CryptoLib contract
/// and serializes given BLS12-381 point into byte array.
pub fn bls12381_serialize(g: Bls12381Point) -> Vec<u8> {
    neogointernal::call_with_token(HASH, "bls12381Serialize", contract::CallFlag::NONE_FLAG, g).unwrap()
}

/// Bls12381Deserialize calls `bls12381Deserialize` method of native CryptoLib
/// contract and deserializes given BLS12-381 point from byte array.
pub fn bls12381_deserialize(data: &[u8]) -> Bls12381Point {
    neogointernal::call_with_token(HASH, "bls12381Deserialize", contract::CallFlag::NONE_FLAG, data).unwrap()
}

/// Bls12381Equal calls `bls12381Equal` method of native CryptoLib contract and
/// checks whether two BLS12-381 points are equal.
pub fn bls12381_equal(x: Bls12381Point, y: Bls12381Point) -> bool {
    neogointernal::call_with_token(HASH, "bls12381Equal", contract::CallFlag::NONE_FLAG, x, y).unwrap()
}

/// Bls12381Add calls `bls12381Add` method of native CryptoLib contract and
/// performs addition operation over two BLS12-381 points.
pub fn bls12381_add(x: Bls12381Point, y: Bls12381Point) -> Bls12381Point {
    neogointernal::call_with_token(HASH, "bls12381Add", contract::CallFlag::NONE_FLAG, x, y).unwrap()
}

/// Bls12381Mul calls `bls12381Mul` method of native CryptoLib contract and
/// performs multiplication operation over BLS12-381 point and the given scalar
/// multiplicator. The multiplicator is the serialized LE representation of the
/// field element stored on 4 words (uint64) with 32-bytes length. The last
/// argument denotes whether the multiplicator should be negative.
pub fn bls12381_mul(x: Bls12381Point, mul: &[u8], neg: bool) -> Bls12381Point {
    neogointernal::call_with_token(HASH, "bls12381Mul", contract::CallFlag::NONE_FLAG, x, mul, neg).unwrap()
}

/// Bls12381Pairing calls `bls12381Pairing` method of native CryptoLib contract and
/// performs pairing operation over two BLS12-381 points which must be G1 and G2 either
/// in Affine or Jacobian forms. The result of this operation is GT point.
pub fn bls12381_pairing(g1: Bls12381Point, g2: Bls12381Point) -> Bls12381Point {
    neogointernal::call_with_token(HASH, "bls12381Pairing", contract::CallFlag::NONE_FLAG, g1, g2).unwrap()
}

/// Keccak256 calls `keccak256` method of native CryptoLib contract and
/// computes Keccak256 hash of b.
pub fn keccak256(b: &[u8]) -> interop::Hash256 {
    neogointernal::call_with_token(HASH, "keccak256", contract::CallFlag::NONE_FLAG, b).unwrap()
}
