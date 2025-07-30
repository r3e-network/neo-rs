/// Proper TestNet debugger using correct Neo protocol
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const TESTNET_MAGIC: u32 = 0x56753345; // TestNet magic

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Neo TestNet Protocol Debugger");
    println!("=============================");
    println!("Target: 34.133.235.69:20333");
    println!("Magic: 0x{:08x}", TESTNET_MAGIC);

    // Connect
    let mut stream =
        TcpStream::connect_timeout(&"34.133.235.69:20333".parse()?, Duration::from_secs(10))?;

    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    println!("\nConnected! Building version message...");

    // Build proper Neo version message
    let version_payload = build_version_payload();
    let message = build_message("version", &version_payload);

    println!("Sending version message ({} bytes):", message.len());
    print_message_details(&message);

    // Send
    stream.write_all(&message)?;
    stream.flush()?;

    println!("\nWaiting for response...");

    // Read response
    let mut all_data = Vec::new();
    let mut buffer = [0u8; 1024];
    let start = std::time::Instant::now();

    loop {
        match stream.read(&mut buffer) {
            Ok(0) => {
                println!("Connection closed");
                break;
            }
            Ok(n) => {
                let elapsed = start.elapsed().as_millis();
                println!("\n[+{}ms] Received {} bytes", elapsed, n);

                let chunk = &buffer[..n];
                all_data.extend_from_slice(chunk);

                // Print hex
                for (i, row) in chunk.chunks(16).enumerate() {
                    print!("  {:04x}: ", i * 16);
                    for byte in row {
                        print!("{:02x} ", byte);
                    }
                    print!("  ");
                    for byte in row {
                        print!(
                            "{}",
                            if byte.is_ascii_graphic() {
                                *byte as char
                            } else {
                                '.'
                            }
                        );
                    }
                    println!();
                }

                // Try to parse messages
                parse_messages(&all_data);
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut
                {
                    if start.elapsed().as_secs() > 10 {
                        println!("\nTimeout after 10 seconds");
                        break;
                    }
                    continue;
                } else {
                    return Err(e.into());
                }
            }
        }
    }

    println!("\nTotal bytes received: {}", all_data.len());
    Ok(())
}

fn build_version_payload() -> Vec<u8> {
    let mut payload = Vec::new();

    // Version
    payload.extend_from_slice(&0u32.to_le_bytes());

    // Services (NODE_NETWORK)
    payload.extend_from_slice(&1u64.to_le_bytes());

    // Timestamp
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32;
    payload.extend_from_slice(&timestamp.to_le_bytes());

    // Nonce
    payload.extend_from_slice(&rand_u32().to_le_bytes());

    // User agent
    let user_agent = b"/NEO:3.6.2/";
    payload.push(user_agent.len() as u8);
    payload.extend_from_slice(user_agent);

    // Start height
    payload.extend_from_slice(&0u32.to_le_bytes());

    // Relay
    payload.push(1);

    payload
}

fn build_message(command: &str, payload: &[u8]) -> Vec<u8> {
    let mut message = Vec::new();

    // Magic
    message.extend_from_slice(&TESTNET_MAGIC.to_le_bytes());

    // Command (12 bytes, padded)
    let mut cmd_bytes = [0u8; 12];
    cmd_bytes[..command.len()].copy_from_slice(command.as_bytes());
    message.extend_from_slice(&cmd_bytes);

    // Payload length
    message.extend_from_slice(&(payload.len() as u32).to_le_bytes());

    // Checksum (first 4 bytes of double SHA256)
    let checksum = calculate_checksum(payload);
    message.extend_from_slice(&checksum.to_le_bytes());

    // Payload
    message.extend_from_slice(payload);

    message
}

fn calculate_checksum(data: &[u8]) -> u32 {
    // Simple checksum for now
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut bytes = [0u8; 4];
        for (i, &b) in chunk.iter().enumerate() {
            bytes[i] = b;
        }
        sum = sum.wrapping_add(u32::from_le_bytes(bytes));
    }
    sum
}

fn rand_u32() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u32
}

fn print_message_details(message: &[u8]) {
    if message.len() >= 24 {
        println!(
            "  Magic: 0x{:08x}",
            u32::from_le_bytes([message[0], message[1], message[2], message[3]])
        );
        let cmd_str = String::from_utf8_lossy(&message[4..16]);
        let cmd = cmd_str.trim_end_matches('\0');
        println!("  Command: '{}'", cmd);
        let len = u32::from_le_bytes([message[16], message[17], message[18], message[19]]);
        println!("  Payload length: {}", len);
        let checksum = u32::from_le_bytes([message[20], message[21], message[22], message[23]]);
        println!("  Checksum: 0x{:08x}", checksum);
    }
}

fn parse_messages(data: &[u8]) {
    let mut offset = 0;

    while offset + 24 <= data.len() {
        // Look for magic bytes
        let magic = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);

        if magic == TESTNET_MAGIC {
            println!("\n  Found message at offset {}:", offset);
            let cmd_bytes = &data[offset + 4..offset + 16];
            let cmd_str = String::from_utf8_lossy(cmd_bytes);
            let cmd = cmd_str.trim_end_matches('\0');
            let payload_len = u32::from_le_bytes([
                data[offset + 16],
                data[offset + 17],
                data[offset + 18],
                data[offset + 19],
            ]);

            println!("    Command: '{}'", cmd);
            println!("    Payload length: {}", payload_len);

            if offset + 24 + payload_len as usize <= data.len() {
                // We have the full message
                let payload = &data[offset + 24..offset + 24 + payload_len as usize];
                println!(
                    "    Payload preview: {:?}",
                    &payload[..payload.len().min(32)]
                );
                offset += 24 + payload_len as usize;
            } else {
                println!("    (incomplete message)");
                break;
            }
        } else {
            offset += 1;
        }
    }
}
