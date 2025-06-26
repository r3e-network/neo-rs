/// Simple test for improved Neo 3 message handling
use std::net::SocketAddr;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter("debug").init();

    println!("🧪 Simple Neo 3 Message Handling Test");
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
        println!("\n📡 Testing connection to: {}", seed);

        match test_simple_connection(seed).await {
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

    println!("\n🎯 Simple message handling test completed!");
    Ok(())
}

async fn test_simple_connection(address: &str) -> Result<(), Box<dyn std::error::Error>> {
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

    // Test our improved message reading logic directly
    for i in 1..=3 {
        println!("📥 Reading message #{} using improved logic...", i);

        match tokio::time::timeout(
            std::time::Duration::from_secs(15),
            read_message_improved(&mut stream),
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
                    "   First 32 bytes: {:02x?}",
                    &message_data[..std::cmp::min(32, message_data.len())]
                );

                if message_data.len() >= 2 {
                    println!(
                        "   Detected format: flags=0x{:02x}, command=0x{:02x}",
                        message_data[0], message_data[1]
                    );
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

/// Our improved message reading logic (simplified version)
async fn read_message_improved(
    stream: &mut TcpStream,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
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
            return read_neo3_after_framing(stream).await;
        } else {
            println!(
                "   ⚠️  No magic number found (0x{:08x}), treating as raw message",
                magic
            );
            // Put the framing bytes back into the message and read more
            let mut message_bytes = framing_bytes.to_vec();

            // Read additional data
            let mut additional_bytes = vec![0u8; 64]; // Read up to 64 more bytes
            match stream.read(&mut additional_bytes).await {
                Ok(n) => {
                    additional_bytes.truncate(n);
                    message_bytes.extend_from_slice(&additional_bytes);
                    println!("   Read {} additional bytes for non-standard message", n);
                }
                Err(_) => {
                    println!("   Could not read additional bytes");
                }
            }

            return Ok(message_bytes);
        }
    } else {
        // This is likely a raw Neo3 message (flags + command + payload)
        // The first byte we read is the flags byte
        println!(
            "   ⚠️  Raw Neo3 message detected (flags=0x{:02x})",
            first_byte[0]
        );
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

    println!(
        "   Neo3 header: flags=0x{:02x}, command=0x{:02x}",
        neo3_header[0], neo3_header[1]
    );

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
    println!(
        "   Raw Neo3 header: flags=0x{:02x}, command=0x{:02x}",
        flags, command_byte[0]
    );

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

    println!(
        "   Complete message: {} bytes (header: 2, length: {}, payload: {})",
        neo3_message_bytes.len(),
        total_length_bytes,
        payload_length
    );

    Ok(neo3_message_bytes)
}
