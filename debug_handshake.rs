use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Debug handshake with Neo N3 TestNet node");
    
    // Connect to a Neo N3 TestNet seed node
    let address = "34.133.235.69:20333";
    println!("📡 Connecting to {}", address);
    
    let mut stream = match TcpStream::connect_timeout(
        &address.parse()?,
        Duration::from_secs(10)
    ) {
        Ok(stream) => {
            println!("✅ TCP connection established");
            stream
        }
        Err(e) => {
            println!("❌ TCP connection failed: {}", e);
            return Err(e.into());
        }
    };
    
    // Set read timeout
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    
    // Create a simple Neo N3 version message
    // This is a minimal version message based on Neo N3 protocol
    let mut version_msg = Vec::new();
    
    // Header: magic (4) + command (12) + length (4) + checksum (4) = 24 bytes
    let magic: u32 = 0x3554334e; // Neo N3 TestNet magic (N5T3)
    let command = "version\0\0\0\0\0"; // 12 bytes, null padded
    
    // Payload fields (simplified)
    let version: u32 = 0x03060000; // Version 3.6.0
    let services: u64 = 1; // NODE_NETWORK
    let timestamp: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let port: u16 = 20333;
    let nonce: u32 = 0x12345678;
    let user_agent = "neo-rs/0.1.0";
    let start_height: u32 = 0;
    let relay: bool = true;
    
    // Build payload
    let mut payload = Vec::new();
    payload.extend_from_slice(&version.to_le_bytes());
    payload.extend_from_slice(&services.to_le_bytes());
    payload.extend_from_slice(&timestamp.to_le_bytes());
    payload.extend_from_slice(&port.to_le_bytes());
    payload.extend_from_slice(&nonce.to_le_bytes());
    
    // Variable length string (user agent)
    let user_agent_bytes = user_agent.as_bytes();
    if user_agent_bytes.len() < 0xFD {
        payload.push(user_agent_bytes.len() as u8);
    } else {
        // For longer strings, would need different encoding
        payload.push(user_agent_bytes.len() as u8);
    }
    payload.extend_from_slice(user_agent_bytes);
    
    payload.extend_from_slice(&start_height.to_le_bytes());
    payload.push(if relay { 1 } else { 0 });
    
    // Simple checksum (not real SHA256, but for testing)
    let checksum: u32 = payload.iter().map(|&b| b as u32).sum();
    
    // Build complete message
    version_msg.extend_from_slice(&magic.to_le_bytes());
    version_msg.extend_from_slice(command.as_bytes());
    version_msg.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    version_msg.extend_from_slice(&checksum.to_le_bytes());
    version_msg.extend_from_slice(&payload);
    
    println!("📤 Sending version message ({} bytes)", version_msg.len());
    println!("   Magic: 0x{:08x}", magic);
    println!("   Command: {}", command.trim_end_matches('\0'));
    println!("   Payload length: {}", payload.len());
    println!("   Checksum: 0x{:08x}", checksum);
    
    // Send version message
    if let Err(e) = stream.write_all(&version_msg) {
        println!("❌ Failed to send version message: {}", e);
        return Err(e.into());
    }
    
    println!("✅ Version message sent, waiting for response...");
    
    // Read response
    let mut buffer = [0u8; 1024];
    match stream.read(&mut buffer) {
        Ok(bytes_read) => {
            println!("📥 Received {} bytes", bytes_read);
            
            // Print raw bytes for debugging
            print!("   Raw bytes: ");
            for i in 0..std::cmp::min(40, bytes_read) {
                print!("{:02x} ", buffer[i]);
            }
            println!();
            
            if bytes_read >= 24 {
                // Parse header
                let response_magic = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
                let response_command = std::str::from_utf8(&buffer[4..16])
                    .unwrap_or("invalid")
                    .trim_end_matches('\0');
                let response_length = u32::from_le_bytes([buffer[16], buffer[17], buffer[18], buffer[19]]);
                let response_checksum = u32::from_le_bytes([buffer[20], buffer[21], buffer[22], buffer[23]]);
                
                println!("   Response magic: 0x{:08x}", response_magic);
                println!("   Response command: {}", response_command);
                println!("   Response length: {}", response_length);
                println!("   Response checksum: 0x{:08x}", response_checksum);
                
                // Verify magic number
                if response_magic == magic {
                    println!("✅ Magic number matches");
                } else {
                    println!("❌ Magic number mismatch: expected 0x{:08x}, got 0x{:08x}", magic, response_magic);
                }
                
                // Check if it's a version or verack response
                if response_command == "version" {
                    println!("✅ Received version response from peer");
                } else if response_command == "verack" {
                    println!("✅ Received verack response from peer");
                } else {
                    println!("⚠️  Unexpected response command: {}", response_command);
                }
                
                println!("🎉 Basic handshake communication successful!");
            } else {
                println!("❌ Response too short: {} bytes", bytes_read);
            }
        }
        Err(e) => {
            println!("❌ Failed to read response: {}", e);
            return Err(e.into());
        }
    }
    
    println!("🏁 Handshake debug complete");
    Ok(())
}