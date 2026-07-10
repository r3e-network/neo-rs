// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License

//! Fuzz target for P2P message parsing.
//!
//! This fuzzer targets the inbound P2P message pipeline to find:
//! - Malformed message headers that cause panics
//! - Invalid compression that causes crashes
//! - Payload size attacks causing OOM
//! - Invalid command types / malformed typed payloads causing crashes

#![no_main]

use libfuzzer_sys::fuzz_target;
use neo_network::NetworkMessage;

fuzz_target!(|data: &[u8]| {
    // `NetworkMessage::from_bytes` exercises the full inbound parse:
    // - envelope parsing (flags, command, var-int payload length)
    // - LZ4 decompression with the 0x02000000 size bound (OOM guard)
    // - the typed payload decode (ProtocolMessage::deserialize_payload)
    // It should return an error for malformed input but never panic.
    if let Ok(message) = NetworkMessage::from_bytes(data) {
        // Round-trip serialize, both with and without compression.
        let _ = message.to_bytes(false);
        let _ = message.to_bytes(true);
    }
});
