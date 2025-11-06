// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

#![cfg_attr(not(feature = "std"), no_std)]

//! Core primitives and utilities shared across the Neo Rust implementation.
//!
//! The crate intentionally keeps dependencies light so it can be used from
//! both the host environment and `no_std` contexts. It exposes strongly typed
//! byte wrappers, hashing helpers, a binary codec and a Merkle tree utility.
//! Higher level crates – including the networking and storage layers –
//! rely on these building blocks for deterministic serialisation.
//!
//! Design goals and responsibilities are documented in
//! `docs/specs/neo-modules.md#neo-base`.

#[cfg(all(feature = "std", feature = "enclave"))]
compile_error!("feature 'std' and 'enclave' cannot be enabled both");

extern crate alloc;

pub mod bytes;
pub mod encoding;
pub mod hash;
pub mod merkle;
pub mod uint;

#[cfg(feature = "std")]
pub mod time;

pub use bytes::Bytes;
pub use encoding::{
    read_varint, write_varint, DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite, SliceReader,
};
pub use hash::{
    double_sha256, hash160, keccak256, murmur128, murmur32, ripemd160, sha256, sha512, Hash160,
    Hash256,
};
pub use uint::{AddressError, AddressVersion, UInt160, UInt256};

#[cfg(feature = "derive")]
pub use neo_proc_macros::{NeoDecode as NeoDecodeDerive, NeoEncode as NeoEncodeDerive};

#[cfg(all(test, feature = "derive"))]
mod derive_tests {
    use super::{Bytes, NeoDecode, NeoDecodeDerive, NeoEncode, NeoEncodeDerive, SliceReader};
    use crate as neo_base;
    use alloc::vec::Vec;

    #[derive(Clone, PartialEq, Debug, NeoEncodeDerive, NeoDecodeDerive)]
    struct SampleStruct {
        id: u32,
        name: Bytes,
    }

    #[derive(Clone, PartialEq, Debug, NeoEncodeDerive, NeoDecodeDerive)]
    enum SampleEnum {
        Unit,
        Tuple(u8, bool),
        Struct { value: u64 },
    }

    #[test]
    fn derive_struct_roundtrip() {
        let item = SampleStruct {
            id: 7,
            name: Bytes::from(b"neo".as_slice()),
        };
        let mut buf = Vec::new();
        item.neo_encode(&mut buf);
        let mut reader = SliceReader::new(buf.as_slice());
        let decoded = SampleStruct::neo_decode(&mut reader).unwrap();
        assert_eq!(decoded, item);
    }

    #[test]
    fn derive_enum_roundtrip() {
        let variants = [
            SampleEnum::Unit,
            SampleEnum::Tuple(1, true),
            SampleEnum::Struct { value: 99 },
        ];

        for variant in variants {
            let mut buf = Vec::new();
            variant.neo_encode(&mut buf);
            let mut reader = SliceReader::new(buf.as_slice());
            let decoded = SampleEnum::neo_decode(&mut reader).unwrap();
            assert_eq!(decoded, variant);
        }
    }
}
