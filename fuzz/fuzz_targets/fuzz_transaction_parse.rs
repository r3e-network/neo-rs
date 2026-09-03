// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License

//! Fuzz target for transaction deserialization.
//!
//! This fuzzer targets the transaction parsing code to find crash/panic inputs
//! that could lead to security vulnerabilities.

#![no_main]

use libfuzzer_sys::fuzz_target;
use neo_io::{MemoryReader, Serializable};
use neo_core::network::p2p::payloads::transaction::Transaction;

fuzz_target!(|data: &[u8]| {
    // Fuzz transaction deserialization from raw bytes
    // This is a critical attack surface as malformed transactions
    // could crash the node or cause resource exhaustion
    
    let mut reader = MemoryReader::new(data);
    
    // Attempt to deserialize a transaction from fuzzed input
    // We expect this to either succeed or return an error,
    // but it should never panic
    let _ = <Transaction as Serializable>::deserialize(&mut reader);
});
