use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("=== Neo P2P Protocol Debug ===\n");
    
    // Test connection to Neo TestNet node
    let addr = "34.133.235.69:20333";
    println!("Connecting to {}...", addr);
    
    match TcpStream::connect(addr).await {
        Ok(mut stream) => {
            println!("✓ Connected successfully!\n");
            
            // First, let's just read what the node sends without sending anything
            println!("Reading initial data from node (if any)...");
            let mut initial_buf = vec![0u8; 1024];
            match tokio::time::timeout(Duration::from_secs(2), stream.read(&mut initial_buf)).await {
                Ok(Ok(n)) if n > 0 => {
                    println!("Received {} bytes without sending anything:", n);
                    print_hex_dump(&initial_buf[..n]);
                },
                _ => {
                    println!("No initial data received from node (this is expected).\n");
                }
            }
            
            // Now let's send a properly formatted version message
            println!("Creating version message...");
            let version_msg = create_version_message();
            println!("Version message ({} bytes):", version_msg.len());
            print_hex_dump(&version_msg);
            
            // Send the version message
            println!("\nSending version message...");
            match stream.write_all(&version_msg).await {
                Ok(_) => println!("✓ Sent successfully!"),
                Err(e) => {
                    println!("✗ Failed to send: {}", e);
                    return;
                }
            }
            
            // Read response
            println!("\nReading response...");
            let mut response_buf = vec![0u8; 1024];
            match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut response_buf)).await {
                Ok(Ok(n)) if n > 0 => {
                    println!("Received {} bytes:", n);
                    print_hex_dump(&response_buf[..n]);
                    
                    // Try to parse as Neo message
                    if n >= 24 {
                        println!("\nParsing as Neo message header:");
                        parse_neo_header(&response_buf[..24]);
                    }
                },
                Ok(Ok(_)) => println!("Connection closed by peer"),
                Ok(Err(e)) => println!("Read error: {}", e),
                Err(_) => println!("Timeout waiting for response"),
            }
        }
        Err(e) => {
            println!("✗ Failed to connect: {}", e);
        }
    }
}

fn create_version_message() -> Vec<u8> {
    let mut msg = Vec::new();
    
    // Header (24 bytes)
    // Magic: TestNet = 0x74746E41
    msg.extend_from_slice(&0x74746E41u32.to_le_bytes());
    
    // Command: "version" (12 bytes, zero-padded)
    let mut command = [0u8; 12];
    command[..7].copy_from_slice(b"version");
    msg.extend_from_slice(&command);
    
    // Calculate payload first
    let payload = create_version_payload();
    
    // Length
    msg.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    
    // Checksum (double SHA256 of payload)
    use sha2::{Digest, Sha256};
    let hash1 = Sha256::digest(&payload);
    let hash2 = Sha256::digest(&hash1);
    msg.extend_from_slice(&hash2[..4]);
    
    // Payload
    msg.extend_from_slice(&payload);
    
    msg
}

fn create_version_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    
    // Version (4 bytes)
    payload.extend_from_slice(&0u32.to_le_bytes());
    
    // Services (8 bytes)
    payload.extend_from_slice(&1u64.to_le_bytes());
    
    // Timestamp (8 bytes)
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    payload.extend_from_slice(&timestamp.to_le_bytes());
    
    // Port (2 bytes)
    payload.extend_from_slice(&20333u16.to_le_bytes());
    
    // Nonce (4 bytes)
    payload.extend_from_slice(&12345u32.to_le_bytes());
    
    // User agent (var string)
    let user_agent = b"/Neo-Rust:0.1.0/";
    payload.push(user_agent.len() as u8); // Length byte
    payload.extend_from_slice(user_agent);
    
    // Start height (4 bytes)
    payload.extend_from_slice(&0u32.to_le_bytes());
    
    // Relay (1 byte)
    payload.push(1); // true
    
    payload
}

fn print_hex_dump(data: &[u8]) {
    for (i, chunk) in data.chunks(16).enumerate() {
        print!("{:04x}: ", i * 16);
        for byte in chunk {
            print!("{:02x} ", byte);
        }
        
        // ASCII representation
        print!("  |");
        for byte in chunk {
            if *byte >= 32 && *byte < 127 {
                print!("{}", *byte as char);
            } else {
                print!(".");
            }
        }
        println!("|");
    }
}

fn parse_neo_header(header: &[u8]) {
    if header.len() < 24 {
        println!("Header too short!");
        return;
    }
    
    let magic = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    println!("  Magic: 0x{:08x}", magic);
    
    let command_bytes = &header[4..16];
    let command = std::str::from_utf8(command_bytes)
        .unwrap_or("INVALID")
        .trim_end_matches('\0');
    println!("  Command: '{}'", command);
    
    let length = u32::from_le_bytes([header[16], header[17], header[18], header[19]]);
    println!("  Payload length: {} bytes", length);
    
    let checksum = u32::from_le_bytes([header[20], header[21], header[22], header[23]]);
    println!("  Checksum: 0x{:08x}", checksum);
}