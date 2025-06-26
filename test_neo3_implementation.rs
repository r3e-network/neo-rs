use neo_network::messages::{
    header::{MessageHeader, MessageFlags},
    network::NetworkMessage,
    protocol::ProtocolMessage,
};

fn main() {
    println!("=== Testing Neo 3 Protocol Implementation ===\n");
    
    // Test 1: Create a version message
    let version_payload = ProtocolMessage::Version {
        version: 0,
        services: 1,
        timestamp: 1234567890,
        port: 20333,
        nonce: 12345,
        user_agent: "/Neo-Rust:0.1.0/".to_string(),
        start_height: 0,
        relay: true,
    };
    
    let message = NetworkMessage::new(version_payload);
    
    println!("Created version message:");
    println!("  Command: 0x{:02x}", message.header.command);
    println!("  Flags: {:?}", message.header.flags);
    println!("  Length: {} bytes", message.header.length);
    
    // Test 2: Serialize the message
    let bytes = message.to_bytes().unwrap();
    println!("\nSerialized message ({} bytes):", bytes.len());
    print_hex(&bytes);
    
    // Test 3: Deserialize the message
    let deserialized = NetworkMessage::from_bytes(&bytes).unwrap();
    println!("\nDeserialized message:");
    println!("  Command: 0x{:02x}", deserialized.header.command);
    println!("  Length: {} bytes", deserialized.header.length);
    
    // Test 4: Create a verack message
    let verack = NetworkMessage::new(ProtocolMessage::Verack);
    let verack_bytes = verack.to_bytes().unwrap();
    
    println!("\nVerack message ({} bytes):", verack_bytes.len());
    print_hex(&verack_bytes);
    
    // Test 5: Verify Neo 3 header format
    println!("\nNeo 3 Message Format:");
    println!("  Byte 0: Flags (0x{:02x})", verack_bytes[0]);
    println!("  Byte 1: Command (0x{:02x})", verack_bytes[1]);
    println!("  Byte 2: Length (0x{:02x})", verack_bytes[2]);
    
    println!("\n✅ Neo 3 protocol implementation test completed!");
}

fn print_hex(data: &[u8]) {
    for (i, chunk) in data.chunks(16).enumerate() {
        print!("  {:04x}: ", i * 16);
        for byte in chunk {
            print!("{:02x} ", byte);
        }
        println!();
    }
}