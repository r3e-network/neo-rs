// Test with real Neo node using our fixed protocol implementation
use std::net::TcpStream;
use std::io::{Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

fn create_version_message() -> Vec<u8> {
    let version = 0u32;
    let services = 1u64;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let port = 20333u16;
    let nonce = 0x12345678u32; // Fixed nonce for testing
    let user_agent = "neo-rs/0.1.0";
    let start_height = 0u32;
    let relay = true;
    
    let mut payload = Vec::new();
    
    // Serialize version message using our fixed format
    payload.extend_from_slice(&version.to_le_bytes());
    payload.extend_from_slice(&services.to_le_bytes());
    payload.extend_from_slice(&timestamp.to_le_bytes());
    payload.extend_from_slice(&port.to_le_bytes());
    payload.extend_from_slice(&nonce.to_le_bytes());
    
    // User agent as var_bytes (fixed from var_string)
    let user_agent_bytes = user_agent.as_bytes();
    payload.push(user_agent_bytes.len() as u8);
    payload.extend_from_slice(user_agent_bytes);
    
    payload.extend_from_slice(&start_height.to_le_bytes());
    payload.push(if relay { 1 } else { 0 });
    
    payload
}

fn create_message_header(command: &str, payload: &[u8]) -> Vec<u8> {
    let magic = 0x3554334e_u32; // Neo N3 TestNet magic
    
    // For simplicity, we'll use a placeholder checksum for now
    // In a real implementation, this would be double SHA256
    let checksum = 0u32;
    
    let mut header = Vec::new();
    header.extend_from_slice(&magic.to_le_bytes());
    
    // Command as 12-byte zero-padded string
    let mut command_bytes = [0u8; 12];
    let cmd_bytes = command.as_bytes();
    command_bytes[..cmd_bytes.len()].copy_from_slice(cmd_bytes);
    header.extend_from_slice(&command_bytes);
    
    header.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    header.extend_from_slice(&checksum.to_le_bytes());
    
    header
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔌 Connecting to Neo N3 TestNet node...");
    
    // Connect to a known Neo TestNet node
    let mut stream = TcpStream::connect("34.133.235.69:20333")?;
    println!("✅ Connected successfully");
    
    // Create version message with fixed user_agent serialization
    let version_payload = create_version_message();
    let header = create_message_header("version", &version_payload);
    
    println!("📦 Sending version message...");
    println!("   Header size: {} bytes", header.len());
    println!("   Payload size: {} bytes", version_payload.len());
    
    // Send complete message
    stream.write_all(&header)?;
    stream.write_all(&version_payload)?;
    stream.flush()?;
    
    println!("📤 Message sent, waiting for response...");
    
    // Read response
    let mut response = [0u8; 1024];
    let n = stream.read(&mut response)?;
    
    if n > 0 {
        println!("📥 Received {} bytes!", n);
        println!("First 64 bytes: {:02x?}", &response[..std::cmp::min(64, n)]);
        
        // Parse response header if we got enough bytes
        if n >= 24 {
            let response_magic = u32::from_le_bytes([
                response[0], response[1], response[2], response[3]
            ]);
            let command_bytes = &response[4..16];
            let command_string = String::from_utf8_lossy(command_bytes);
            let command = command_string.trim_end_matches('\0');
            let payload_len = u32::from_le_bytes([
                response[16], response[17], response[18], response[19]
            ]);
            
            println!("🔍 Response analysis:");
            println!("   Magic: 0x{:08x} (should be 0x3554334e)", response_magic);
            println!("   Command: '{}'", command);
            println!("   Payload length: {}", payload_len);
            
            if response_magic == 0x3554334e {
                println!("🎉 SUCCESS! Received valid Neo N3 response!");
                
                if command == "version" {
                    println!("✅ Peer responded with version message - handshake working!");
                } else if command == "verack" {
                    println!("✅ Peer sent verack - protocol handshake successful!");
                } else {
                    println!("📋 Peer sent: '{}'", command);
                }
                
                return Ok(());
            }
        }
    } else {
        println!("❌ No response received");
    }
    
    println!("⚠️  Response didn't match expected Neo protocol format");
    Ok(())
}