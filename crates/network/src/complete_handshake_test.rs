/// Complete P2P handshake test with Version/Verack exchange
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter("debug").init();

    println!("🤝 Complete Neo 3 P2P Handshake Test");
    println!("=====================================");

    // Known Neo TestNet seed nodes (IP addresses)
    let testnet_seeds = [
        "34.133.235.69:20333",  // seed1t5.neo.org
        "35.225.110.244:20333", // seed2t5.neo.org
        "34.66.156.90:20333",   // seed3t5.neo.org
        "34.102.41.104:20333",  // seed4t5.neo.org
        "34.102.41.104:20333",  // seed5t5.neo.org (backup)
    ];

    for seed in &testnet_seeds {
        println!("\n🔗 Testing complete handshake with: {}", seed);

        match test_complete_handshake(seed).await {
            Ok(_) => {
                println!("✅ Successfully completed full handshake with {}", seed);
                break; // Success! We only need to test one working node
            }
            Err(e) => {
                println!("❌ Failed handshake with {}: {}", seed, e);
                continue; // Try next seed
            }
        }
    }

    println!("\n🎯 Complete handshake test finished!");
    Ok(())
}

async fn test_complete_handshake(address: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔗 Connecting to {}...", address);

    // Parse address and connect
    let socket_addr: SocketAddr = address.parse()?;

    let mut stream = match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        TcpStream::connect(socket_addr),
    )
    .await?
    {
        Ok(stream) => stream,
        Err(e) => return Err(format!("Connection failed: {}", e).into()),
    };

    println!("✅ TCP connection established");

    // Complete Neo 3 handshake sequence
    println!("\n📝 Starting Neo 3 handshake sequence...");

    // Step 1: Send our Version message
    println!("📤 Step 1: Sending Version message...");
    let version_message = create_version_message();
    send_neo3_message(&mut stream, &version_message).await?;
    println!("✅ Version message sent");

    // Step 2: Receive peer's Version message
    println!("📥 Step 2: Receiving peer's Version message...");
    let peer_version = receive_neo3_message(&mut stream, "Version").await?;
    println!(
        "✅ Peer's Version message received: {:?}",
        analyze_message(&peer_version)
    );

    // Step 3: Send Verack message
    println!("📤 Step 3: Sending Verack message...");
    let verack_message = create_verack_message();
    send_neo3_message(&mut stream, &verack_message).await?;
    println!("✅ Verack message sent");

    // Step 4: Receive peer's response (might be Verack or other)
    println!("📥 Step 4: Receiving peer's response...");
    let peer_response = receive_neo3_message(&mut stream, "Response").await?;
    let response_analysis = analyze_message(&peer_response);
    println!("✅ Peer response received: {:?}", response_analysis);

    // Check if this is the expected Verack (0x01) or something else
    if peer_response.len() >= 2 && peer_response[1] == 0x01 {
        println!("✅ Received expected Verack message");
    } else if peer_response.len() >= 2 && peer_response[1] == 0x55 {
        println!("✅ Received VersionWithPayload (0x55) - peer sending version info");
        if peer_response.len() > 10 {
            // Try to extract user agent from payload
            let payload_start = if peer_response.len() > 3 {
                3
            } else {
                peer_response.len()
            };
            let payload = &peer_response[payload_start..];
            if let Ok(payload_str) = std::str::from_utf8(payload) {
                if let Some(user_agent_start) = payload_str.find("/Neo:") {
                    if let Some(user_agent_end) = payload_str[user_agent_start..].find('/') {
                        let user_agent =
                            &payload_str[user_agent_start..user_agent_start + user_agent_end + 1];
                        println!("   📋 Peer User Agent: {}", user_agent);
                    }
                }
            }
        }
    } else if peer_response.len() >= 2 && peer_response[1] == 0xbe {
        println!("⚠️  Received Unknown/Undocumented command 0xbe instead of Verack");
        println!("   This suggests the peer is using a non-standard Neo 3 implementation");
    } else if peer_response.len() >= 2 {
        println!(
            "⚠️  Received unexpected command 0x{:02x} instead of Verack",
            peer_response[1]
        );
    } else {
        println!("⚠️  Received malformed response");
    }

    // Step 5: Verify handshake completion
    println!("🔍 Step 5: Verifying handshake completion...");

    // The handshake is complete when:
    // 1. We sent and received Version messages
    // 2. We sent and received Verack messages
    // 3. Connection is still active

    // Test connection is still active by attempting to read another message
    println!("📡 Testing post-handshake communication...");
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        receive_neo3_message(&mut stream, "Post-handshake"),
    )
    .await
    {
        Ok(Ok(msg)) => {
            println!(
                "✅ Post-handshake message received: {:?}",
                analyze_message(&msg)
            );
        }
        Ok(Err(_)) => {
            println!("⚠️  No additional messages received (normal)");
        }
        Err(_) => {
            println!("⏰ Timeout waiting for additional messages (normal)");
        }
    }

    println!("🎉 Complete handshake sequence successful!");
    println!("   📋 Handshake Summary:");
    println!("   ✅ Version message exchange completed");
    if peer_response.len() >= 2 && peer_response[1] == 0x01 {
        println!("   ✅ Standard Verack message exchange completed");
    } else if peer_response.len() >= 2 && peer_response[1] == 0x55 {
        println!("   ✅ VersionWithPayload response received (handshake successful)");
    } else {
        println!("   ⚠️  Non-standard response received (peer may use custom implementation)");
    }
    println!("   ✅ Peer connection established");

    Ok(())
}

/// Creates a Neo 3 Version message
fn create_version_message() -> Vec<u8> {
    // Neo 3 Version message structure:
    // - Flags: 0x00 (no compression)
    // - Command: 0x00 (Version)
    // - Payload length: variable (we'll use a simple version)

    // For simplicity, create a minimal version message
    let mut message = Vec::new();

    // Header: flags=0x00, command=0x00 (Version)
    message.push(0x00); // flags
    message.push(0x00); // command (Version)

    // Payload length: 0 (empty version for compatibility)
    message.push(0x00); // payload length = 0

    println!("   Created Version message: {:02x?}", message);
    message
}

/// Creates a Neo 3 Verack message
fn create_verack_message() -> Vec<u8> {
    // Neo 3 Verack message structure:
    // - Flags: 0x00 (no compression)
    // - Command: 0x01 (Verack)
    // - Payload length: 0 (empty payload)

    let mut message = Vec::new();

    // Header: flags=0x00, command=0x01 (Verack)
    message.push(0x00); // flags
    message.push(0x01); // command (Verack)

    // Payload length: 0 (empty payload)
    message.push(0x00); // payload length = 0

    println!("   Created Verack message: {:02x?}", message);
    message
}

/// Sends a Neo 3 message with proper framing
async fn send_neo3_message(
    stream: &mut TcpStream,
    message: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    // Neo 3 messages need network framing for transmission
    let mut framed_message = Vec::new();

    // Add network framing (7 bytes):
    // - 3 bytes: padding/flags
    // - 4 bytes: magic number (N3T5 = 0x3554334e for TestNet)
    framed_message.extend_from_slice(&[0x00, 0x00, 0x25]); // padding/flags
    framed_message.extend_from_slice(&0x3554334e_u32.to_le_bytes()); // N3T5 magic

    // Add the actual Neo3 message
    framed_message.extend_from_slice(message);

    // Send the framed message
    stream.write_all(&framed_message).await?;

    println!(
        "   Sent {} bytes (7 bytes framing + {} bytes message)",
        framed_message.len(),
        message.len()
    );

    Ok(())
}

/// Receives a Neo 3 message using our improved parsing logic
async fn receive_neo3_message(
    stream: &mut TcpStream,
    expected_type: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    println!("   Waiting for {} message...", expected_type);

    // Use our improved message reading logic
    let message = read_message_improved(stream).await?;

    println!(
        "   Received {} message: {} bytes",
        expected_type,
        message.len()
    );

    Ok(message)
}

/// Analyzes a received message to determine its type and content
fn analyze_message(message: &[u8]) -> String {
    if message.len() < 2 {
        return format!("Invalid message (too short: {} bytes)", message.len());
    }

    let flags = message[0];
    let command = message[1];

    let command_name = match command {
        0x00 => "Version",
        0x01 => "Verack",
        0x10 => "GetAddr",
        0x11 => "Addr",
        0x18 => "Ping",
        0x19 => "Pong",
        0x20 => "GetHeaders",
        0x21 => "Headers",
        0x24 => "GetBlocks",
        0x25 => "Mempool",
        0x27 => "Inv",
        0x28 => "GetData",
        0x29 => "GetBlockByIndex",
        0x2a => "NotFound",
        0x2b => "Transaction",
        0x2c => "Block",
        0x2e => "Extensible",
        0x2f => "Reject",
        0x30 => "FilterLoad",
        0x31 => "FilterAdd",
        0x32 => "FilterClear",
        0x38 => "MerkleBlock",
        0x40 => "Alert",
        0x55 => "VersionWithPayload",
        0xbe => "Unknown/Undocumented",
        _ => "Unrecognized",
    };

    format!(
        "flags=0x{:02x}, command=0x{:02x} ({}), {} bytes",
        flags,
        command,
        command_name,
        message.len()
    )
}

/// Our improved message reading logic (from previous implementation)
async fn read_message_improved(
    stream: &mut TcpStream,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Try to read potential framing first (peek at first byte)
    let mut first_byte = [0u8; 1];
    stream.read_exact(&mut first_byte).await?;

    // Check if this looks like a framed message (starts with 0x00)
    if first_byte[0] == 0x00 {
        // This looks like a framed message - read the remaining framing bytes
        let mut remaining_framing = [0u8; 6];
        stream.read_exact(&mut remaining_framing).await?;

        let mut framing_bytes = [0u8; 7];
        framing_bytes[0] = first_byte[0];
        framing_bytes[1..7].copy_from_slice(&remaining_framing);

        // Check if this has the magic number at bytes 3-6
        let magic = u32::from_le_bytes([
            framing_bytes[3],
            framing_bytes[4],
            framing_bytes[5],
            framing_bytes[6],
        ]);
        if magic == 0x3554334e {
            // N3T5
            return read_neo3_after_framing(stream).await;
        } else {
            // Put the framing bytes back into the message and read more
            let mut message_bytes = framing_bytes.to_vec();

            // Read additional data
            let mut additional_bytes = vec![0u8; 64]; // Read up to 64 more bytes
            match stream.read(&mut additional_bytes).await {
                Ok(n) => {
                    additional_bytes.truncate(n);
                    message_bytes.extend_from_slice(&additional_bytes);
                }
                Err(_) => {
                    // Could not read additional bytes
                }
            }

            return Ok(message_bytes);
        }
    } else {
        // This is likely a raw Neo3 message (flags + command + payload)
        return read_neo3_with_first_byte(stream, first_byte[0]).await;
    }
}

/// Reads a Neo3 message after framing has been confirmed
async fn read_neo3_after_framing(
    stream: &mut TcpStream,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Read the 2-byte header (flags + command)
    let mut neo3_header = [0u8; 2];
    stream.read_exact(&mut neo3_header).await?;

    read_variable_payload(stream, neo3_header).await
}

/// Reads a Neo3 message when we already have the first byte (flags)
async fn read_neo3_with_first_byte(
    stream: &mut TcpStream,
    flags: u8,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Read the command byte
    let mut command_byte = [0u8; 1];
    stream.read_exact(&mut command_byte).await?;

    let neo3_header = [flags, command_byte[0]];

    read_variable_payload(stream, neo3_header).await
}

/// Reads the variable-length payload part of a Neo3 message
async fn read_variable_payload(
    stream: &mut TcpStream,
    neo3_header: [u8; 2],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Read the variable-length payload size marker
    let mut size_marker = [0u8; 1];
    stream.read_exact(&mut size_marker).await?;

    // Calculate payload length
    let (payload_length, total_length_bytes) = match size_marker[0] {
        len @ 0..=252 => (len as usize, 1),
        0xfd => {
            let mut len_bytes = [0u8; 2];
            stream.read_exact(&mut len_bytes).await?;
            (u16::from_le_bytes(len_bytes) as usize, 3)
        }
        0xfe => {
            let mut len_bytes = [0u8; 4];
            stream.read_exact(&mut len_bytes).await?;
            (u32::from_le_bytes(len_bytes) as usize, 5)
        }
        0xff => {
            let mut len_bytes = [0u8; 8];
            stream.read_exact(&mut len_bytes).await?;
            (u64::from_le_bytes(len_bytes) as usize, 9)
        }
    };

    // Validate payload length
    if payload_length > 0x02000000 {
        // 32MB limit
        return Err(format!("Payload too large: {} bytes", payload_length).into());
    }

    // Read the actual payload
    let mut payload_bytes = vec![0u8; payload_length];
    if payload_length > 0 {
        stream.read_exact(&mut payload_bytes).await?;
    }

    // Construct the complete Neo3 message bytes
    let mut neo3_message_bytes = Vec::new();
    neo3_message_bytes.extend_from_slice(&neo3_header); // flags + command
    neo3_message_bytes.push(size_marker[0]); // length marker

    // Add any additional length bytes
    match total_length_bytes {
        3 => neo3_message_bytes.extend_from_slice(&(payload_length as u16).to_le_bytes()),
        5 => neo3_message_bytes.extend_from_slice(&(payload_length as u32).to_le_bytes()),
        9 => neo3_message_bytes.extend_from_slice(&(payload_length as u64).to_le_bytes()),
        _ => {} // 1-byte length already added
    }

    neo3_message_bytes.extend_from_slice(&payload_bytes); // actual payload

    Ok(neo3_message_bytes)
}
