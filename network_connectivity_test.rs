//! Network Connectivity Test for Neo TestNet
//! Tests basic TCP connectivity to Neo infrastructure

use std::net::{TcpStream, SocketAddr};
use std::time::Duration;
use std::io::{Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🌐 Neo TestNet Network Connectivity Test");
    println!("========================================");
    
    // Use IP addresses instead of hostnames
    let testnet_ips = vec![
        "149.28.51.74:20333",   // seed1t.neo.org
        "149.28.51.75:20333",   // seed2t.neo.org
        "149.28.51.76:20333",   // seed3t.neo.org  
        "149.28.51.77:20333",   // seed4t.neo.org
        "149.28.51.78:20333",   // seed5t.neo.org
    ];
    
    let mut working_peers = Vec::new();
    
    for ip in &testnet_ips {
        println!("🔌 Testing TCP connection to {}", ip);
        
        match test_tcp_connection(ip) {
            Ok(response) => {
                working_peers.push(ip);
                println!("✅ Success: {}", response);
            }
            Err(e) => {
                println!("❌ Failed: {}", e);
            }
        }
    }
    
    println!("\n📊 Results:");
    println!("   ✅ Working peers: {}/{}", working_peers.len(), testnet_ips.len());
    
    if !working_peers.is_empty() {
        println!("🎉 Neo TestNet infrastructure is accessible!");
        println!("   Working peers: {:?}", working_peers);
        
        // Test Neo protocol handshake with first working peer
        if let Some(peer) = working_peers.first() {
            println!("\n🤝 Testing Neo protocol handshake with {}...", peer);
            match test_neo_protocol(peer) {
                Ok(msg) => println!("✅ Neo protocol test: {}", msg),
                Err(e) => println!("❌ Neo protocol test failed: {}", e),
            }
        }
    } else {
        println!("⚠️ No peers accessible - network may be restricted");
    }
    
    Ok(())
}

fn test_tcp_connection(address: &str) -> Result<String, Box<dyn std::error::Error>> {
    let addr: SocketAddr = address.parse()?;
    
    match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
        Ok(_stream) => Ok("TCP connection successful".to_string()),
        Err(e) => Err(format!("TCP connection failed: {}", e).into()),
    }
}

fn test_neo_protocol(address: &str) -> Result<String, Box<dyn std::error::Error>> {
    let addr: SocketAddr = address.parse()?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    
    // Create minimal Neo version message
    let mut message = Vec::new();
    
    // TestNet magic
    message.extend_from_slice(&0x3554334Eu32.to_le_bytes());
    
    // Command "version" (12 bytes)
    message.extend_from_slice(b"version\0\0\0\0\0");
    
    // Minimal payload  
    let payload = vec![0u8; 37]; // Minimal version payload
    message.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    message.extend_from_slice(&0u32.to_le_bytes()); // Checksum
    message.extend_from_slice(&payload);
    
    // Send message
    stream.write_all(&message)?;
    
    // Read response
    let mut buffer = [0u8; 100];
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    
    match stream.read(&mut buffer) {
        Ok(bytes) if bytes >= 24 => {
            let response_magic = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
            if response_magic == 0x3554334E {
                Ok(format!("Valid Neo TestNet response ({} bytes)", bytes))
            } else {
                Ok(format!("Response received but invalid magic: 0x{:08X}", response_magic))
            }
        }
        Ok(bytes) => Ok(format!("Short response: {} bytes", bytes)),
        Err(e) => Err(format!("Read timeout: {}", e).into()),
    }
}