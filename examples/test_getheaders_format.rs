use neo_network::messages::network::NetworkMessage;
use neo_network::messages::protocol::ProtocolMessage;

fn main() {
    let get_headers = ProtocolMessage::GetHeaders {
        index_start: 0,
        count: -1,
    };

    match get_headers.to_bytes() {
        Ok(payload_bytes) => {
            println!(
                "GetHeaders payload ({} bytes): {:02x?}",
                payload_bytes.len(),
                payload_bytes
            );

            let message = NetworkMessage::new(get_headers);
            match message.to_bytes() {
                Ok(full_bytes) => {
                    println!("\nFull message ({} bytes):", full_bytes.len());
                    println!("Magic: {:02x?}", &full_bytes[0..4]);
                    println!(
                        "Command: {} ({:02x?})",
                        String::from_utf8_lossy(&full_bytes[4..16]),
                        &full_bytes[4..16]
                    );
                    println!(
                        "Payload length: {:02x?} ({})",
                        &full_bytes[16..20],
                        u32::from_le_bytes([
                            full_bytes[16],
                            full_bytes[17],
                            full_bytes[18],
                            full_bytes[19]
                        ])
                    );
                    println!("Checksum: {:02x?}", &full_bytes[20..24]);
                    println!("Payload: {:02x?}", &full_bytes[24..]);
                }
                Err(e) => println!("Error creating full message: {}", e),
            }
        }
        Err(e) => println!("Error serializing: {}", e),
    }
}
