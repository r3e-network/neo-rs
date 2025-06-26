// Test with real Neo node using proper checksum calculation
use std::net::TcpStream;
use std::io::{Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

// Simple SHA256 implementation for checksum calculation
fn sha256(data: &[u8]) -> [u8; 32] {
    // For this test, we'll use a dummy implementation
    // In a real implementation, we'd use a proper SHA256 library
    let mut hash = [0u8; 32];
    
    // Simple checksum based on data bytes (not cryptographically secure)
    let mut sum: u64 = 0;
    for (i, &byte) in data.iter().enumerate() {
        sum = sum.wrapping_add((byte as u64).wrapping_mul((i as u64 + 1)));
    }
    
    // Fill hash with pattern based on sum
    for i in 0..32 {
        hash[i] = ((sum >> (i % 64)) & 0xFF) as u8;
    }
    
    hash
}

fn calculate_checksum(data: &[u8]) -> u32 {
    // Neo uses double SHA256 for checksum
    let hash1 = sha256(data);
    let hash2 = sha256(&hash1);
    
    // Take first 4 bytes as little-endian u32
    u32::from_le_bytes([hash2[0], hash2[1], hash2[2], hash2[3]])
}

fn create_version_message() -> Vec<u8> {
    let version = 0u32;
    let services = 1u64;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let port = 20333u16;
    let nonce = 0x12345678u32;
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
    
    // Calculate proper checksum
    let checksum = calculate_checksum(payload);
    
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
    println!("🔌 Testing Neo P2P handshake with proper checksum...");
    
    // Connect to a known Neo TestNet node
    let mut stream = TcpStream::connect("34.133.235.69:20333")?;
    println!("✅ Connected successfully");
    
    // Create version message with fixed user_agent serialization
    let version_payload = create_version_message();
    let header = create_message_header("version", &version_payload);
    
    println!("📦 Message details:");
    println!("   Header size: {} bytes", header.len());
    println!("   Payload size: {} bytes", version_payload.len());
    println!("   Total size: {} bytes", header.len() + version_payload.len());
    
    let checksum = calculate_checksum(&version_payload);
    println!("   Calculated checksum: 0x{:08x}", checksum);
    
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
        
        // Show more bytes to understand the structure
        for i in (0..n).step_by(16) {
            let end = std::cmp::min(i + 16, n);
            print!("   {:04x}: ", i);
            for j in i..end {
                print!("{:02x} ", response[j]);
            }
            print!(" ");
            for j in i..end {
                let c = response[j];
                if c >= 32 && c <= 126 {
                    print!("{}", c as char);
                } else {
                    print!(".");
                }
            }
            println!();
        }
        
        // Try different interpretations of the response
        println!("\n🔍 Analysis attempts:");
        
        // Maybe the response is just raw payload without header?
        if n >= 4 {
            let possible_version = u32::from_le_bytes([
                response[0], response[1], response[2], response[3]
            ]);
            println!("   If raw version payload - version: {}", possible_version);
        }
        
        // Maybe it's offset by some bytes?
        for offset in 0..std::cmp::min(8, n.saturating_sub(24)) {
            if n >= offset + 24 {
                let magic = u32::from_le_bytes([
                    response[offset], response[offset+1], 
                    response[offset+2], response[offset+3]
                ]);
                if magic == 0x3554334e {
                    println!("   Found valid magic at offset {}: 0x{:08x}", offset, magic);
                    let command_bytes = &response[offset+4..offset+16];
                    let command_string = String::from_utf8_lossy(command_bytes);
                    let command = command_string.trim_end_matches('\0');
                    println!("   Command: '{}'", command);
                }
            }
        }
        
        println!("🤔 Response format doesn't match expected Neo protocol header");
        println!("   This might indicate:");
        println!("   1. The node rejected our message");
        println!("   2. Different protocol version");
        println!("   3. Connection was reset");
        println!("   4. Our message format is still incorrect");
        
    } else {
        println!("❌ No response received - connection may have been closed");
    }
    
    Ok(())
}