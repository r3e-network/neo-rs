/// Analyze the TestNet response we received
fn main() {
    let response_hex = "0000254e33543500000000ecb689689c5a521c0b2f4e656f3a332e382e322f0210becd7c00016d4f";
    let response_bytes = hex_to_bytes(response_hex);
    
    println!("Analyzing TestNet Response");
    println!("==========================");
    println!("Total bytes: {}", response_bytes.len());
    println!("Hex: {}", response_hex);
    println!();
    
    // Try different interpretations
    println!("Interpretation 1: Looking for magic bytes at different offsets");
    for i in 0..response_bytes.len().saturating_sub(3) {
        let potential_magic = &response_bytes[i..i+4];
        println!("  Offset {}: {:02x} {:02x} {:02x} {:02x} = 0x{:08x}", 
            i, 
            potential_magic[0], potential_magic[1], potential_magic[2], potential_magic[3],
            u32::from_le_bytes([potential_magic[0], potential_magic[1], potential_magic[2], potential_magic[3]])
        );
    }
    
    println!("\nInterpretation 2: Parse as version response starting at offset 0");
    if response_bytes.len() >= 32 {
        // Try parsing as a version payload
        let mut offset = 0;
        
        // Version
        let version = u32::from_le_bytes([
            response_bytes[offset], response_bytes[offset+1], 
            response_bytes[offset+2], response_bytes[offset+3]
        ]);
        offset += 4;
        println!("  Version: {}", version);
        
        // Network ID string (4 bytes)
        let network_id = &response_bytes[offset..offset+4];
        println!("  Network ID: {:?} (as string: '{}')", 
            network_id, 
            String::from_utf8_lossy(network_id)
        );
        offset += 4;
        
        // Magic bytes?
        let magic = u32::from_le_bytes([
            response_bytes[offset], response_bytes[offset+1], 
            response_bytes[offset+2], response_bytes[offset+3]
        ]);
        offset += 4;
        println!("  Magic/Unknown: 0x{:08x}", magic);
        
        // Continue parsing...
        println!("  Remaining bytes from offset {}: {:?}", offset, &response_bytes[offset..]);
        
        // Try to find user agent
        for i in offset..response_bytes.len() {
            if response_bytes[i] == b'/' {
                // Found potential user agent start
                if i > 0 {
                    let len = response_bytes[i-1] as usize;
                    if i + len <= response_bytes.len() {
                        let user_agent = &response_bytes[i..i+len];
                        println!("  Potential user agent at {}: len={}, value='{}'", 
                            i-1, len, String::from_utf8_lossy(user_agent));
                    }
                }
            }
        }
    }
    
    println!("\nInterpretation 3: Looking for TestNet magic (0x56753345)");
    let testnet_magic = 0x56753345u32;
    let testnet_bytes = testnet_magic.to_le_bytes();
    println!("  TestNet magic bytes (LE): {:02x} {:02x} {:02x} {:02x}", 
        testnet_bytes[0], testnet_bytes[1], testnet_bytes[2], testnet_bytes[3]);
    
    // Check if "N3T5" appears anywhere
    for i in 0..response_bytes.len().saturating_sub(3) {
        if &response_bytes[i..i+4] == b"N3T5" {
            println!("  Found 'N3T5' at offset {}", i);
        }
    }
    
    println!("\nInterpretation 4: Raw ASCII decode");
    println!("  Full ASCII: '{}'", 
        response_bytes.iter()
            .map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' })
            .collect::<String>()
    );
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i+2], 16).unwrap())
        .collect()
}