//! Network handshake test focusing on P2P protocol compatibility

use neo_config::NetworkType;
use neo_core::UInt160;
use neo_network::messages::protocol::ProtocolMessage;
use neo_network::{NetworkConfig, NodeInfo};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
#[ignore] // Run with --ignored flag for real network tests
async fn test_handshake_with_real_node() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔌 Testing handshake with real Neo N3 node...");

    // Create version message using our protocol implementation
    let node_info = NodeInfo {
        user_agent: "neo-rs/0.1.0".to_string(),
        protocol_version: 3,
        network: neo_config::NetworkType::TestNet,
        port: 20333,
    };

    let version_msg = ProtocolMessage::version(&node_info, 20333, true);
    let version_bytes = version_msg.to_bytes()?;

    println!("📦 Version message size: {} bytes", version_bytes.len());

    // Create message header for Neo N3 TestNet
    let magic = 0x3554334e_u32.to_le_bytes(); // TestNet magic
    let command = b"version\0\0\0\0\0"; // 12 bytes, zero-padded
    let length = (version_bytes.len() as u32).to_le_bytes();

    // Calculate checksum (double SHA256)
    use sha2::{Digest, Sha256};
    let hash1 = Sha256::digest(&version_bytes);
    let hash2 = Sha256::digest(&hash1);
    let checksum = u32::from_le_bytes([hash2[0], hash2[1], hash2[2], hash2[3]]).to_le_bytes();

    // Construct complete message
    let mut message = Vec::new();
    message.extend_from_slice(&magic);
    message.extend_from_slice(command);
    message.extend_from_slice(&length);
    message.extend_from_slice(&checksum);
    message.extend_from_slice(&version_bytes);

    println!("📡 Total message size: {} bytes", message.len());

    // Connect to Neo TestNet node
    let mut stream = TcpStream::connect("34.133.235.69:20333").await?;
    println!("✅ Connected to Neo TestNet node");

    // Send version message
    stream.write_all(&message).await?;
    println!("📤 Sent version message");

    // Read response
    let mut response_buffer = [0u8; 2048];
    let n = stream.read(&mut response_buffer).await?;
    println!("📥 Received {} bytes response", n);

    if n > 0 {
        println!("🎉 Successfully received response from Neo node!");
        println!(
            "First 64 bytes: {:02x?}",
            &response_buffer[..std::cmp::min(64, n)]
        );

        // Try to parse the response as a Neo message
        if n >= 24 {
            let response_magic = u32::from_le_bytes([
                response_buffer[0],
                response_buffer[1],
                response_buffer[2],
                response_buffer[3],
            ]);
            let response_command =
                String::from_utf8_lossy(&response_buffer[4..16]).trim_end_matches('\0');
            let response_length = u32::from_le_bytes([
                response_buffer[16],
                response_buffer[17],
                response_buffer[18],
                response_buffer[19],
            ]);

            println!("🔍 Response magic: 0x{:08x}", response_magic);
            println!("🔍 Response command: '{}'", response_command);
            println!("🔍 Response payload length: {}", response_length);

            // Check if this looks like a valid Neo response
            if response_magic == 0x3554334e {
                println!("✅ Received valid Neo N3 TestNet response!");
                return Ok(());
            }
        }
    }

    Err("Did not receive expected response".into())
}

#[tokio::test]
async fn test_version_message_serialization() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing version message serialization...");

    let node_info = NodeInfo {
        user_agent: "neo-rs/0.1.0".to_string(),
        protocol_version: 3,
        network: NetworkType::TestNet,
        port: 20333,
    };

    let version_msg = ProtocolMessage::version(&node_info, 20333, true);
    let serialized = version_msg.to_bytes()?;

    println!("📊 Serialized version message:");
    println!("   Size: {} bytes", serialized.len());
    println!(
        "   Data: {:02x?}",
        &serialized[..std::cmp::min(64, serialized.len())]
    );

    // Verify the structure matches what we expect
    assert!(
        !serialized.is_empty(),
        "Serialized message should not be empty"
    );
    assert!(
        serialized.len() > 20,
        "Version message should be substantial size"
    );

    println!("✅ Version message serialization test passed");
    Ok(())
}
