use neo_network::messages::network::NetworkMessage;
use neo_network::messages::protocol::ProtocolMessage;

fn main() {
    println!("Testing Neo N3 version message format");

    // Create a version message
    let version_msg = ProtocolMessage::Version {
        version: 3,
        services: 1,
        timestamp: 1738257472,
        port: 10333,
        nonce: 12345,
        user_agent: "Neo:3.8.2".to_string(),
        start_height: 0,
        relay: true,
    };

    // Serialize it
    match version_msg.to_bytes() {
        Ok(bytes) => {
            println!("Version message serialized: {} bytes", bytes.len());
            println!("Hex: {}", hex::encode(&bytes));
            println!("First 50 bytes: {:02x?}", &bytes[..bytes.len().min(50)]);

            // Try to create a NetworkMessage and serialize
            let net_msg = NetworkMessage::new(version_msg.clone());
            match net_msg.to_bytes() {
                Ok(net_bytes) => {
                    println!("\nNetworkMessage serialized: {} bytes", net_bytes.len());
                    println!(
                        "First 50 bytes: {:02x?}",
                        &net_bytes[..net_bytes.len().min(50)]
                    );
                }
                Err(e) => {
                    println!("Failed to serialize NetworkMessage: {}", e);
                }
            }
        }
        Err(e) => {
            println!("Failed to serialize version message: {}", e);
        }
    }

    // Test parsing the actual Neo N3 format
    // This is a real version message captured from Neo N3
    let neo3_bytes =
        hex::decode("250000004e454f330300000040c99c670f3930390a4e656f3a332e382e322f00000000016d")
            .unwrap();
    println!(
        "\n\nTesting parsing of real Neo N3 version message: {} bytes",
        neo3_bytes.len()
    );
    println!("Hex: {}", hex::encode(&neo3_bytes));

    // Try to parse it
    match NetworkMessage::from_neo3_real_bytes(&neo3_bytes) {
        Ok(msg) => {
            println!("Successfully parsed Neo N3 message!");
            println!("Command: {:?}", msg.command());
            if let ProtocolMessage::Version {
                version,
                user_agent,
                start_height,
                ..
            } = &msg.payload
            {
                println!("Version: {}", version);
                println!("User agent: {}", user_agent);
                println!("Start height: {}", start_height);
            }
        }
        Err(e) => {
            println!("Failed to parse Neo N3 message: {}", e);
        }
    }
}
