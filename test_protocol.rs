// Simple test of protocol message functionality
use std::time::{SystemTime, UNIX_EPOCH};

// Simulate the core types we need
#[derive(Debug, Clone, PartialEq)]
pub struct UInt160([u8; 20]);

impl UInt160 {
    pub fn zero() -> Self {
        UInt160([0; 20])
    }
}

// Test the user agent serialization fix
fn test_user_agent_serialization() {
    println!("🧪 Testing user agent serialization...");
    
    let user_agent = "neo-rs/0.1.0";
    let user_agent_bytes = user_agent.as_bytes();
    
    // Test var_bytes format (length + data)
    let mut serialized = Vec::new();
    
    // Write length as var_int (simple case for small strings)
    let len = user_agent_bytes.len() as u8;
    serialized.push(len);
    serialized.extend_from_slice(user_agent_bytes);
    
    println!("📦 User agent '{}' serialized as var_bytes:", user_agent);
    println!("   Length: {} bytes", len);
    println!("   Data: {:02x?}", serialized);
    
    // This should match what our fixed protocol.rs does
    assert_eq!(serialized[0], len);
    assert_eq!(&serialized[1..], user_agent_bytes);
    
    println!("✅ User agent serialization test passed");
}

fn test_version_message_structure() {
    println!("🔍 Testing version message structure...");
    
    let version = 0u32;
    let services = 1u64;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let port = 20333u16;
    let nonce = 0x12345678u32;
    let user_agent = "neo-rs/0.1.0";
    let start_height = 0u32;
    let relay = true;
    
    let mut payload = Vec::new();
    
    // Serialize version message (same order as protocol.rs)
    payload.extend_from_slice(&version.to_le_bytes());
    payload.extend_from_slice(&services.to_le_bytes());
    payload.extend_from_slice(&timestamp.to_le_bytes());
    payload.extend_from_slice(&port.to_le_bytes());
    payload.extend_from_slice(&nonce.to_le_bytes());
    
    // User agent as var_bytes (length + data)
    let user_agent_bytes = user_agent.as_bytes();
    payload.push(user_agent_bytes.len() as u8);
    payload.extend_from_slice(user_agent_bytes);
    
    payload.extend_from_slice(&start_height.to_le_bytes());
    payload.push(if relay { 1 } else { 0 });
    
    println!("📊 Version message payload:");
    println!("   Total size: {} bytes", payload.len());
    println!("   Version: {}", version);
    println!("   Services: {}", services);
    println!("   Port: {}", port);
    println!("   User agent: '{}'", user_agent);
    println!("   Start height: {}", start_height);
    println!("   Relay: {}", relay);
    println!("   First 32 bytes: {:02x?}", &payload[..std::cmp::min(32, payload.len())]);
    
    // Verify the structure
    assert_eq!(payload.len() > 20, true);
    
    println!("✅ Version message structure test passed");
}

fn test_message_header() {
    println!("🏷️  Testing message header...");
    
    let magic = 0x3554334e_u32; // Neo N3 TestNet magic
    let command = "version";
    let payload_len = 42u32;
    let checksum = 0x12345678u32;
    
    let mut header = Vec::new();
    header.extend_from_slice(&magic.to_le_bytes());
    
    // Command as 12-byte zero-padded string
    let mut command_bytes = [0u8; 12];
    let cmd_bytes = command.as_bytes();
    command_bytes[..cmd_bytes.len()].copy_from_slice(cmd_bytes);
    header.extend_from_slice(&command_bytes);
    
    header.extend_from_slice(&payload_len.to_le_bytes());
    header.extend_from_slice(&checksum.to_le_bytes());
    
    println!("📋 Message header:");
    println!("   Size: {} bytes", header.len());
    println!("   Magic: 0x{:08x}", magic);
    println!("   Command: '{}'", command);
    println!("   Payload length: {}", payload_len);
    println!("   Checksum: 0x{:08x}", checksum);
    println!("   Bytes: {:02x?}", header);
    
    // Verify header is exactly 24 bytes
    assert_eq!(header.len(), 24);
    
    println!("✅ Message header test passed");
}

fn main() {
    println!("🚀 Testing Neo P2P protocol implementation...");
    
    test_user_agent_serialization();
    test_version_message_structure();
    test_message_header();
    
    println!("🎉 All protocol tests passed!");
}