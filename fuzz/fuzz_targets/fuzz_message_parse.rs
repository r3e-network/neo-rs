// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License

//! Fuzz target for P2P message parsing.
//!
//! This fuzzer targets the P2P message deserialization to find:
//! - Malformed message headers that cause panics
//! - Invalid compression that causes crashes
//! - Payload size attacks causing OOM
//! - Invalid command types causing crashes

#![no_main]

use libfuzzer_sys::fuzz_target;
use neo_io::{MemoryReader, Serializable};
use neo_core::network::p2p::message::Message;

fuzz_target!(|data: &[u8]| {
    // Fuzz P2P message deserialization
    // The Message structure contains:
    // - flags (1 byte): compression flags
    // - command (1 byte): message type
    // - payload: variable-length byte array (potentially compressed)
    
    // Minimum size for a valid message header:
    // - 1 byte flags
    // - 1 byte command
    // - 1 byte payload length (minimum)
    if data.len() < 3 {
        return;
    }
    
    let mut reader = MemoryReader::new(data);
    
    // Attempt to deserialize a message from fuzzed input
    // This exercises:
    // - Message header parsing
    // - Compression/decompression (LZ4)
    // - Payload length validation
    let result = <Message as Serializable>::deserialize(&mut reader);
    
    match result {
        Ok(message) => {
            // If we successfully parsed a message, try to access its properties
            // and convert to protocol message
            let _ = message.is_compressed();
            let _ = message.payload();
            let _ = message.to_protocol_message();
            
            // Try to serialize it back
            let _ = message.to_bytes(false);
        }
        Err(_) => {
            // Error is expected for malformed input
            // We just want to ensure no panics occur
        }
    }
});
