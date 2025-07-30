/// TestNet debugger v2 - handles the actual TestNet protocol
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Neo TestNet Debugger v2");
    println!("=======================");
    println!("Connecting to Neo TestNet at 34.133.235.69:20333...");

    // Connect with timeout
    let mut stream =
        TcpStream::connect_timeout(&"34.133.235.69:20333".parse()?, Duration::from_secs(10))?;

    // Set read timeout
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;

    println!("Connected! Sending version message...");

    // Build version message for TestNet
    // Based on the response analysis, TestNet uses a different protocol
    let mut message = Vec::new();

    // Protocol version (0 as u32)
    message.extend_from_slice(&0u32.to_le_bytes());

    // Network identifier "N3T5"
    message.extend_from_slice(b"N3T5");

    // Unknown field (0 as u32)
    message.extend_from_slice(&0u32.to_le_bytes());

    // Timestamp
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as u32;
    message.extend_from_slice(&timestamp.to_le_bytes());

    // Nonce
    message.extend_from_slice(&42u32.to_le_bytes());

    // User agent length and string
    let user_agent = b"/NEO:3.6.2/";
    message.push(user_agent.len() as u8);
    message.extend_from_slice(user_agent);

    // Additional fields (based on response pattern)
    message.push(0x02); // Unknown byte
    message.push(0x10); // Unknown byte

    // Start height
    message.extend_from_slice(&0u32.to_le_bytes());

    // Relay flag
    message.push(1);

    // Additional data?
    message.extend_from_slice(&0x4f6du16.to_le_bytes());

    println!("Sending message ({} bytes):", message.len());
    println!("Hex: {}", to_hex(&message));

    // Send the message
    stream.write_all(&message)?;
    stream.flush()?;

    println!("\nListening for responses...");

    // Read responses with better buffering
    let mut all_data = Vec::new();
    let mut buffer = [0u8; 4096];
    let start_time = std::time::Instant::now();

    loop {
        match stream.read(&mut buffer) {
            Ok(0) => {
                println!("Connection closed by peer");
                break;
            }
            Ok(n) => {
                println!(
                    "\nReceived {} bytes at +{}ms:",
                    n,
                    start_time.elapsed().as_millis()
                );
                let chunk = &buffer[..n];
                println!("Hex: {}", to_hex(chunk));

                all_data.extend_from_slice(chunk);

                // Analyze the chunk
                analyze_chunk(chunk, all_data.len() - n);
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut
                {
                    println!("\nTimeout after {}ms", start_time.elapsed().as_millis());
                    break;
                } else {
                    return Err(e.into());
                }
            }
        }
    }

    println!("\n=== Session Summary ===");
    println!("Total bytes received: {}", all_data.len());
    if !all_data.is_empty() {
        println!("Complete response hex:\n{}", to_hex(&all_data));
        analyze_complete_response(&all_data);
    }

    Ok(())
}

fn analyze_chunk(chunk: &[u8], offset: usize) {
    println!(
        "ASCII: '{}'",
        chunk
            .iter()
            .map(|&b| if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '.'
            })
            .collect::<String>()
    );

    // Look for patterns
    for i in 0..chunk.len() {
        // Look for "N3T5"
        if i + 4 <= chunk.len() && &chunk[i..i + 4] == b"N3T5" {
            println!(
                "  Found 'N3T5' at chunk offset {} (absolute offset {})",
                i,
                offset + i
            );
        }

        // Look for user agent patterns
        if chunk[i] == b'/' {
            if i > 0 && i + chunk[i - 1] as usize <= chunk.len() {
                let len = chunk[i - 1] as usize;
                let agent = &chunk[i..i + len];
                if agent.iter().all(|&b| b.is_ascii_graphic() || b == b' ') {
                    println!(
                        "  Found user agent at chunk offset {}: '{}'",
                        i - 1,
                        String::from_utf8_lossy(agent)
                    );
                }
            }
        }
    }
}

fn analyze_complete_response(data: &[u8]) {
    println!("\n=== Response Analysis ===");

    if data.len() >= 4 {
        println!(
            "First 4 bytes as u32 (LE): {}",
            u32::from_le_bytes([data[0], data[1], data[2], data[3]])
        );
    }

    if data.len() >= 8 && &data[4..8] == b"N3T5" {
        println!("Network identifier confirmed: N3T5 (TestNet)");

        if data.len() >= 40 {
            println!("\nParsing as TestNet version message:");
            let mut offset = 0;

            println!(
                "  Version: {}",
                u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3]
                ])
            );
            offset += 4;

            println!(
                "  Network: {}",
                String::from_utf8_lossy(&data[offset..offset + 4])
            );
            offset += 4;

            println!(
                "  Unknown field: 0x{:08x}",
                u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3]
                ])
            );
            offset += 4;

            println!(
                "  Timestamp: {}",
                u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3]
                ])
            );
            offset += 4;

            println!(
                "  Nonce: {}",
                u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3]
                ])
            );
            offset += 4;

            if offset < data.len() {
                let agent_len = data[offset] as usize;
                offset += 1;

                if offset + agent_len <= data.len() {
                    println!(
                        "  User Agent: '{}'",
                        String::from_utf8_lossy(&data[offset..offset + agent_len])
                    );
                    offset += agent_len;
                }
            }

            if offset < data.len() {
                println!(
                    "  Remaining bytes from offset {}: {:?}",
                    offset,
                    &data[offset..]
                );
            }
        }
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}
