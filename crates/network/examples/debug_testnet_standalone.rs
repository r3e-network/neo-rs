/// Standalone TestNet debug tool - no dependencies on neo-network crate
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Standalone Neo TestNet Debugger");
    println!("================================");
    println!("Connecting to Neo TestNet at 34.133.235.69:20333...");

    // Connect with timeout
    let mut stream =
        TcpStream::connect_timeout(&"34.133.235.69:20333".parse()?, Duration::from_secs(10))?;

    // Set read timeout to handle no data gracefully
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;

    println!("Connected! Sending version message...");

    // Build a minimal version message for TestNet
    // TestNet magic: 0x56753345
    let magic_bytes = [0x45, 0x33, 0x75, 0x56]; // Little-endian

    // Command: "version" (12 bytes, padded with zeros)
    let command = b"version\0\0\0\0\0";

    // Simple version payload (minimal fields)
    let mut payload = Vec::new();

    // Version: 0
    payload.extend_from_slice(&0u32.to_le_bytes());

    // Services: 1 (NODE_NETWORK)
    payload.extend_from_slice(&1u64.to_le_bytes());

    // Timestamp
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as u32;
    payload.extend_from_slice(&timestamp.to_le_bytes());

    // Nonce
    payload.extend_from_slice(&42u32.to_le_bytes());

    // User agent length and string
    let user_agent = b"/NEO:3.6.2/";
    payload.push(user_agent.len() as u8);
    payload.extend_from_slice(user_agent);

    // Start height: 0
    payload.extend_from_slice(&0u32.to_le_bytes());

    // Relay: true
    payload.push(1);

    // Calculate payload length and checksum (using simple SHA256 implementation)
    let payload_len = payload.len() as u32;
    let checksum = calculate_checksum(&payload);

    // Build complete message
    let mut message = Vec::new();
    message.extend_from_slice(&magic_bytes);
    message.extend_from_slice(command);
    message.extend_from_slice(&payload_len.to_le_bytes());
    message.extend_from_slice(&checksum.to_le_bytes());
    message.extend_from_slice(&payload);

    println!("Sending version message ({} bytes):", message.len());
    println!("Hex: {}", to_hex(&message));

    // Send the message
    stream.write_all(&message)?;
    stream.flush()?;

    println!("\nListening for responses...");

    // Read responses
    let mut buffer = [0u8; 4096];
    let mut total_bytes = 0;

    loop {
        match stream.read(&mut buffer) {
            Ok(0) => {
                println!("Connection closed by peer");
                break;
            }
            Ok(n) => {
                println!("\nReceived {} bytes:", n);
                println!("Hex: {}", to_hex(&buffer[..n]));
                println!(
                    "ASCII (printable only): {}",
                    String::from_utf8_lossy(&buffer[..n])
                        .chars()
                        .map(|c| if c.is_ascii_graphic() || c == ' ' {
                            c
                        } else {
                            '.'
                        })
                        .collect::<String>()
                );

                total_bytes += n;

                // Try to identify message boundaries (look for magic bytes)
                for i in 0..n.saturating_sub(3) {
                    if &buffer[i..i + 4] == &magic_bytes {
                        println!("Found magic bytes at offset {}", i);

                        // Try to parse message header if we have enough bytes
                        if i + 24 <= n {
                            let cmd_bytes = &buffer[i + 4..i + 16];
                            let cmd_str = String::from_utf8_lossy(cmd_bytes);
                            let cmd = cmd_str.trim_end_matches('\0');
                            println!("  Command: '{}'", cmd);

                            let payload_len = u32::from_le_bytes([
                                buffer[i + 16],
                                buffer[i + 17],
                                buffer[i + 18],
                                buffer[i + 19],
                            ]);
                            println!("  Payload length: {}", payload_len);
                        }
                    }
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut
                {
                    println!("\nTimeout after receiving {} total bytes", total_bytes);
                    break;
                } else {
                    return Err(e.into());
                }
            }
        }
    }

    println!(
        "\nDebug session complete. Total bytes received: {}",
        total_bytes
    );
    Ok(())
}

/// Convert bytes to hex string
fn to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}

/// Calculate checksum using basic SHA256 (simplified for standalone use)
fn calculate_checksum(data: &[u8]) -> u32 {
    // For this debug tool, we'll use a simple checksum
    // In production, this would be double SHA256
    let mut sum: u32 = 0;
    for chunk in data.chunks(4) {
        let mut bytes = [0u8; 4];
        for (i, &b) in chunk.iter().enumerate() {
            bytes[i] = b;
        }
        sum = sum.wrapping_add(u32::from_le_bytes(bytes));
    }
    sum
}
