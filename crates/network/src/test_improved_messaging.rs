use neo_network::{NetworkConfig, NetworkMessage, PeerManager, ProtocolVersion};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Test the improved Neo 3 message handling with real TestNet nodes
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter("debug").init();

    println!("🧪 Testing Improved Neo 3 Message Handling");
    println!("==========================================");

    // Known Neo TestNet seed nodes
    let testnet_seeds = [
        "seed1t5.neo.org:20333",
        "seed2t5.neo.org:20333",
        "seed3t5.neo.org:20333",
        "seed4t5.neo.org:20333",
        "seed5t5.neo.org:20333",
    ];

    for seed in &testnet_seeds {
        println!("\n📡 Testing connection to: {}", seed);

        match test_neo_connection(seed).await {
            Ok(_) => {
                println!(
                    "✅ Successfully tested improved message handling with {}",
                    seed
                );
                break; // Success! We only need to test one working node
            }
            Err(e) => {
                println!("❌ Failed to connect to {}: {}", seed, e);
                continue; // Try next seed
            }
        }
    }

    println!("\n🎯 Improved message handling test completed!");
    Ok(())
}

async fn test_neo_connection(address: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔗 Connecting to {}...", address);

    // Parse address
    let socket_addr: SocketAddr = address.parse()?;

    // Create TCP connection
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

    // Test multiple message reads using our improved logic
    for i in 1..=3 {
        println!(
            "📥 Reading message #{} using improved message handling...",
            i
        );

        match tokio::time::timeout(
            std::time::Duration::from_secs(15),
            read_neo_message(&mut stream),
        )
        .await?
        {
            Ok(message_data) => {
                println!(
                    "✅ Message #{} successfully read: {} bytes",
                    i,
                    message_data.len()
                );
                println!(
                    "   Data: {:02x?}",
                    &message_data[..std::cmp::min(32, message_data.len())]
                );

                // Try to parse the message
                match NetworkMessage::from_bytes(&message_data) {
                    Ok(parsed) => {
                        println!("   ✅ Successfully parsed as: {:?}", parsed.command());
                    }
                    Err(e) => {
                        println!("   ⚠️  Parsing failed: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("❌ Failed to read message #{}: {}", i, e);
                if i == 1 {
                    return Err(e.into()); // First message is critical
                }
                break; // Later messages failing is less critical
            }
        }
    }

    Ok(())
}

/// Use our improved message reading logic
async fn read_neo_message(stream: &mut TcpStream) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Try to read potential framing first (peek at first byte)
    let mut first_byte = [0u8; 1];
    stream.read_exact(&mut first_byte).await?;

    println!("   First byte: 0x{:02x}", first_byte[0]);

    // Check if this looks like a framed message (starts with 0x00)
    if first_byte[0] == 0x00 {
        // This looks like a framed message - read the remaining framing bytes
        let mut remaining_framing = [0u8; 6];
        stream.read_exact(&mut remaining_framing).await?;

        let mut framing_bytes = [0u8; 7];
        framing_bytes[0] = first_byte[0];
        framing_bytes[1..7].copy_from_slice(&remaining_framing);

        println!("   Framing bytes: {:02x?}", framing_bytes);

        // Check if this has the magic number at bytes 3-6
        let magic = u32::from_le_bytes([
            framing_bytes[3],
            framing_bytes[4],
            framing_bytes[5],
            framing_bytes[6],
        ]);
        if magic == 0x3554334e {
            // N3T5
            println!("   ✅ Found N3T5 magic - reading framed Neo3 message");
            return read_neo3_message_after_framing(stream).await;
        } else {
            println!("   ⚠️  No magic number found, treating as raw message");
            // Put the framing bytes back into the message and parse as Neo3
            let mut message_bytes = framing_bytes.to_vec();

            // Read additional data to determine message structure
            let additional = read_remaining_neo3_message(stream).await?;
            message_bytes.extend_from_slice(&additional);

            return Ok(message_bytes);
        }
    } else {
        // This is likely a raw Neo3 message (flags + command + payload)
        // The first byte we read is the flags byte
        println!("   ⚠️  Raw Neo3 message detected");
        return read_neo3_message_with_first_byte(stream, first_byte[0]).await;
    }
}

/// Reads a Neo3 message after framing has been confirmed
async fn read_neo3_message_after_framing(
    stream: &mut TcpStream,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Read the 2-byte header (flags + command)
    let mut neo3_header = [0u8; 2];
    stream.read_exact(&mut neo3_header).await?;

    println!(
        "   Neo3 header: flags=0x{:02x}, command=0x{:02x}",
        neo3_header[0], neo3_header[1]
    );

    read_variable_length_payload(stream, neo3_header).await
}

/// Reads a Neo3 message when we already have the first byte (flags)
async fn read_neo3_message_with_first_byte(
    stream: &mut TcpStream,
    flags: u8,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Read the command byte
    let mut command_byte = [0u8; 1];
    stream.read_exact(&mut command_byte).await?;

    let neo3_header = [flags, command_byte[0]];
    println!(
        "   Raw Neo3 header: flags=0x{:02x}, command=0x{:02x}",
        flags, command_byte[0]
    );

    read_variable_length_payload(stream, neo3_header).await
}

/// Reads the variable-length payload part of a Neo3 message
async fn read_variable_length_payload(
    stream: &mut TcpStream,
    neo3_header: [u8; 2],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Read the variable-length payload size marker (at least 1 byte)
    let mut size_marker = [0u8; 1];
    stream.read_exact(&mut size_marker).await?;

    // Calculate how many more bytes we need for the length
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

    println!("   Payload length: {} bytes", payload_length);

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

    // Construct the complete Neo3 message bytes for parsing
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

    println!(
        "   Complete message: {} bytes (header: 2, length: {}, payload: {})",
        neo3_message_bytes.len(),
        total_length_bytes,
        payload_length
    );

    Ok(neo3_message_bytes)
}

/// Reads remaining bytes for messages that don't follow standard framing
async fn read_remaining_neo3_message(
    stream: &mut TcpStream,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // For non-standard messages, try to read a reasonable amount of data
    // This is a fallback for unusual message formats
    let mut additional_bytes = vec![0u8; 64]; // Read up to 64 more bytes

    match stream.read(&mut additional_bytes).await {
        Ok(n) => {
            additional_bytes.truncate(n);
            println!("   Read {} additional bytes for non-standard message", n);
            Ok(additional_bytes)
        }
        Err(e) => {
            println!("   Failed to read additional message data: {}", e);
            Ok(Vec::new()) // Return empty if we can't read more
        }
    }
}
