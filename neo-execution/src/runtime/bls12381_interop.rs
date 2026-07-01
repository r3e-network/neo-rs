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

use neo_vm::stack_item::InteropInterface as VmInteropInterface;
use std::any::Any;

/// VM interop wrapper holding a BLS12-381 point's canonical encoding.
#[derive(Debug, Clone)]
pub struct Bls12381Interop {
    bytes: Vec<u8>,
}

impl Bls12381Interop {
    /// Wraps a point's canonical serialization (48 / 96 / 576 bytes).
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// The point's canonical serialization.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl VmInteropInterface for Bls12381Interop {
    fn interface_type(&self) -> &str {
        // The group is implied by the encoding length; this string mirrors the
        // C# runtime object kind for diagnostics only (an InteropInterface is
        // never serialized, so it is not consensus-observable).
        match self.bytes.len() {
            48 => "G1Affine",
            96 => "G2Affine",
            576 => "Gt",
            _ => "Bls12381Point",
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
