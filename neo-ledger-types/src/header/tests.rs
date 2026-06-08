// Copyright (C) 2015-2025 The Neo Project.
//
// header_tests.rs belongs to the neo project and is free software
// distributed under the MIT software license, see the accompanying
// file LICENSE in the main directory of the repository or
// http://www.opensource.org/licenses/mit-license.php for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Pure data-layer tests for [`Header`]. Stateful tests that exercise
//! `DataCache` / `HeaderCache` / native-contract lookup live in
//! `neo-core`'s `tests/` integration tree.

#![allow(clippy::field_reassign_with_default)]

use super::Header;
use crate::Witness;
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::{UInt160, UInt256};
use neo_vm_rs::OpCode;

fn sample_witness() -> Witness {
    Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1.byte()])
}

fn sample_header() -> Header {
    Header::from_parts(
        0,
        UInt256::default(),
        UInt256::default(),
        1_000,
        0,
        0,
        0,
        UInt160::default(),
        sample_witness(),
    )
}

#[test]
fn header_default_is_zero() {
    let h = Header::new();
    assert_eq!(h.version(), 0);
    assert_eq!(h.timestamp(), 0);
    assert_eq!(h.index(), 0);
    assert_eq!(h.primary_index(), 0);
    assert!(h.prev_hash().is_zero());
    assert!(h.merkle_root().is_zero());
    assert!(h.next_consensus().is_zero());
}

#[test]
fn header_setter_invalidates_hash_cache() {
    let mut h = sample_header();
    let _ = h.try_hash().expect("hash ok");
    h.set_index(42);
    assert_eq!(h.index(), 42);
    // hash will be recomputed on next read
    let h2 = h.try_hash().expect("hash ok 2");
    assert_ne!(h2, UInt256::zero());
}

#[test]
fn header_serde_roundtrip() {
    let h = sample_header();
    let mut w = BinaryWriter::new();
    h.serialize(&mut w).expect("serialize");
    let bytes = w.into_bytes();
    let mut r = MemoryReader::new(&bytes);
    let h2 = Header::deserialize(&mut r).expect("deserialize");
    assert_eq!(h.version(), h2.version());
    assert_eq!(h.index(), h2.index());
    assert_eq!(h.timestamp(), h2.timestamp());
    assert_eq!(h.merkle_root(), h2.merkle_root());
    assert_eq!(h.prev_hash(), h2.prev_hash());
    assert_eq!(h.next_consensus(), h2.next_consensus());
    assert_eq!(h.primary_index(), h2.primary_index());
}

#[test]
fn header_hash_is_deterministic() {
    let h1 = sample_header();
    let h2 = sample_header();
    assert_eq!(h1.try_hash().unwrap(), h2.try_hash().unwrap());
}

#[test]
fn header_size_matches_serialized_length() {
    let h = sample_header();
    let mut w = BinaryWriter::new();
    h.serialize(&mut w).expect("serialize");
    let bytes = w.into_bytes();
    assert_eq!(h.size(), bytes.len());
}
