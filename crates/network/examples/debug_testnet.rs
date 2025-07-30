use hex;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    // Calculate payload length and checksum
    let payload_len = payload.len() as u32;
    let checksum = {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(&Sha256::digest(&payload));
        u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]])
    };

    // Build complete message
    let mut message = Vec::new();
    message.extend_from_slice(&magic_bytes);
    message.extend_from_slice(command);
    message.extend_from_slice(&payload_len.to_le_bytes());
    message.extend_from_slice(&checksum.to_le_bytes());
    message.extend_from_slice(&payload);

    println!("Sending version message ({} bytes):", message.len());
    println!("Hex: {}", hex::encode(&message));

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
                println!("Hex: {}", hex::encode(&buffer[..n]));
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
