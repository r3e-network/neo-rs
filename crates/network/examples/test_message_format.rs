use neo_network::{NetworkConfig, NetworkMessage, ProtocolMessage};

fn main() {
    // Create a test version message similar to what the peer manager creates
    let config = NetworkConfig::default(); // Uses correct magic 0x334F454E

    let payload = ProtocolMessage::Version {
        version: 0, // Correct protocol version
        services: 1,
        timestamp: 1722433200, // Fixed timestamp for consistent testing
        port: 10333,
        nonce: 0x12345678, // Fixed nonce for testing
        user_agent: "neo-rs/0.1.0".to_string(),
        start_height: 0,
        relay: true,
    };

    // Create message with correct magic
    let message = NetworkMessage::new_with_magic(payload, config.magic);

    // Serialize to bytes
    match message.to_bytes() {
        Ok(bytes) => {
            println!("Message format analysis:");
            println!("Magic number: 0x{:08X}", config.magic);
            println!("Total message length: {} bytes", bytes.len());
            println!("First 50 bytes: {:02X?}", &bytes[..bytes.len().min(50)]);

            // Check if this is Neo3 format (should start with flags + command)
            if bytes.len() >= 2 {
                println!("Byte 0 (flags): 0x{:02X}", bytes[0]);
                println!("Byte 1 (command): 0x{:02X}", bytes[1]);

                if bytes[0] == 0x00 && bytes[1] == 0x00 {
                    println!("✅ Detected Neo N3 format (flags=0x00, command=0x00 for Version)");
                } else {
                    println!("❌ Not standard Neo N3 format");
                }
            }

            // Analyze the structure
            if bytes.len() >= 4 && bytes[0] == 0x00 && bytes[1] == 0x00 {
                // Check if byte 2 is a length indicator
                let potential_length = bytes[2] as usize;
                println!(
                    "Byte 2 (potential length): {} (0x{:02X})",
                    potential_length, bytes[2]
                );

                if potential_length + 3 == bytes.len() {
                    println!("✅ Confirmed: 3-byte header format (flags + command + length)");
                } else {
                    println!(
                        "❌ Length mismatch: expected {}, got {}",
                        potential_length + 3,
                        bytes.len()
                    );
                }
            }
        }
        Err(e) => {
            println!("Failed to serialize message: {}", e);
        }
    }
}
