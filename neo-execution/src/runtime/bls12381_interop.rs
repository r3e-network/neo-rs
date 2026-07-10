//! `Bls12381Interop` — the VM `InteropInterface` wrapper for a BLS12-381 curve
//! point, mirroring C# `CryptoLib`'s `InteropInterface(G1Affine | G2Affine |
//! Gt)`.
//!
//! Unlike [`IteratorInterop`](crate::iterators::IteratorInterop) (which
//! references a stateful engine-side cursor by id), a BLS point is an immutable
//! value, so this wrapper carries the point's canonical serialization directly
//! (48-byte compressed G1 / 96-byte compressed G2 / 576-byte Gt). The native
//! `CryptoLib` BLS methods round-trip through these bytes via
//! `neo_crypto::Bls12381Point`.
//!
//! Representing the point as a *typed* interop object (rather than a bare
//! `ByteString`) is what preserves C# parity: a `CryptoLib` BLS method only
//! accepts a value produced by `bls12381Deserialize`/`…Add`/`…Mul`/`…Pairing`,
//! exactly as C# binds an `InteropInterface` parameter via `GetInterface<>()`
//! and faults on a plain byte string.

use neo_vm::stack_item::InteropInterface;

/// VM interop wrapper holding a BLS12-381 point's canonical encoding.
pub type Bls12381Interop = InteropInterface;

/// Constructors for BLS12-381 VM interop handles.
pub trait Bls12381InteropExt {
    /// Wraps a point's canonical serialization (48 / 96 / 576 bytes).
    fn new(bytes: Vec<u8>) -> Self;

    /// The point's canonical serialization.
    fn bytes(&self) -> &[u8];
}

impl Bls12381InteropExt for InteropInterface {
    fn new(bytes: Vec<u8>) -> Self {
        InteropInterface::bls12381(bytes)
    }

    fn bytes(&self) -> &[u8] {
        self.bls12381_bytes().unwrap_or(&[])
    }
}
