use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::timeout;
use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub mod local_node;
pub mod message;
pub mod message_command;
pub mod message_flags;
pub mod peer;
pub mod remote_node;
pub mod task_manager;
pub mod task_session;
mod remote_node_protocol_handler;

/// Represents a connection of the P2P network.
pub struct Connection {
    /// The address of the remote node.
    pub remote: SocketAddr,
    /// The address of the local node.
    pub local: SocketAddr,
    stream: TcpStream,
    disconnected: bool,
}

impl Connection {
    /// Connection initial timeout (in seconds) before any package has been accepted.
    const CONNECTION_TIMEOUT_LIMIT_START: u64 = 10;
    /// Connection timeout (in seconds) after every `on_received` event.
    const CONNECTION_TIMEOUT_LIMIT: u64 = 60;

    /// Initializes a new instance of the Connection struct.
    pub fn new(stream: TcpStream, remote: SocketAddr, local: SocketAddr) -> Self {
        Self {
            remote,
            local,
            stream,
            disconnected: false,
        }
    }

    /// Disconnect from the remote node.
    pub async fn disconnect(&mut self, abort: bool) {
        self.disconnected = true;
        if abort {
            self.stream.abort().await.ok();
        } else {
            self.stream.shutdown().await.ok();
        }
    }

    /// Called when a TCP ACK message is received.
    async fn on_ack(&mut self) {
        // Implementation depends on specific requirements
    }

    /// Called when data is received.
    async fn on_data(&mut self, data: BytesMut) {
        // Implementation depends on specific requirements
    }

    /// Main loop for handling incoming messages
    pub async fn run(&mut self) {
        let mut buf = BytesMut::with_capacity(1024);

        loop {
            match timeout(Duration::from_secs(Self::CONNECTION_TIMEOUT_LIMIT), self.stream.read_buf(&mut buf)).await {
                Ok(Ok(0)) => {
                    // Connection closed
                    break;
                }
                Ok(Ok(_)) => {
                    // Data received
                    if let Err(_) = self.on_data(buf.split()).await {
                        self.disconnect(true).await;
                        break;
                    }
                }
                Ok(Err(_)) => {
                    // Error reading from stream
                    self.disconnect(true).await;
                    break;
                }
                Err(_) => {
                    // Timeout
                    self.disconnect(true).await;
                    break;
                }
            }
        }
    }

    /// Sends data to the remote node.
    pub async fn send_data(&mut self, data: BytesMut) -> std::io::Result<()> {
        self.stream.write_all(&data).await
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Example usage
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080").await?;
    let (stream, remote_addr) = listener.accept().await?;
    let local_addr = stream.local_addr()?;

    let mut connection = Connection::new(stream, remote_addr, local_addr);
    connection.run().await;

    Ok(())
}
