//! Simple test to see what Neo nodes send us
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
#[ignore] // Run with --ignored flag for real network tests
async fn test_basic_connection() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to Neo testnet node
    let mut stream = TcpStream::connect("34.133.235.69:20333").await?;
    println!("Connected to Neo testnet node");

    // Create a minimal version message
    let magic = 0x3554334e_u32.to_le_bytes(); // Neo N3 testnet magic
    let command = b"version\0\0\0\0\0"; // 12 bytes, zero-padded
    let length = 38_u32.to_le_bytes(); // Version message is ~38 bytes
    let checksum = 0_u32.to_le_bytes();

    let mut header = Vec::new();
    header.extend_from_slice(&magic);
    header.extend_from_slice(command);
    header.extend_from_slice(&length);
    header.extend_from_slice(&checksum);

    // Create minimal version payload
    let version = 0_u32.to_le_bytes();
    let services = 1_u64.to_le_bytes();
    let timestamp = 1640995200_u64.to_le_bytes();
    let port = 20333_u16.to_le_bytes();
    let nonce = 12345_u32.to_le_bytes();
    let user_agent = b"\x07neo-rs"; // var_string: length + data
    let start_height = 0_u32.to_le_bytes();
    let relay = [1_u8]; // bool true

    let mut payload = Vec::new();
    payload.extend_from_slice(&version);
    payload.extend_from_slice(&services);
    payload.extend_from_slice(&timestamp);
    payload.extend_from_slice(&port);
    payload.extend_from_slice(&nonce);
    payload.extend_from_slice(user_agent);
    payload.extend_from_slice(&start_height);
    payload.extend_from_slice(&relay);

    // Send the version message
    stream.write_all(&header).await?;
    stream.write_all(&payload).await?;
    println!("Sent version message");

    // Try to read response
    let mut buffer = [0u8; 1024];
    let n = stream.read(&mut buffer).await?;
    println!("Received {} bytes: {:02x?}", n, &buffer[..n]);

    Ok(())
}
