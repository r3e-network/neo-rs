use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::timeout;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use bytes::{BytesMut, Bytes};
use actix::prelude::*;

#[derive(Message)]
#[rtype(result = "()")]
struct Close {
    abort: bool,
}

#[derive(Message)]
#[rtype(result = "()")]
struct Ack;

/// Represents a connection of the P2P network.
pub trait Connection: Actor<Context = Context<Self>> {
    /// Connection initial timeout (in seconds) before any package has been accepted.
    const CONNECTION_TIMEOUT_LIMIT_START: u64 = 10;
    /// Connection timeout (in seconds) after every `on_received` event.
    const CONNECTION_TIMEOUT_LIMIT: u64 = 60;

    /// Get the address of the remote node.
    fn remote(&self) -> SocketAddr;

    /// Get the address of the local node.
    fn local(&self) -> SocketAddr;

    /// Get the TcpStream.
    fn stream(&self) -> &TcpStream;

    /// Get mutable reference to the TcpStream.
    fn stream_mut(&mut self) -> &mut TcpStream;

    /// Check if the connection is disconnected.
    fn is_disconnected(&self) -> bool;

    /// Set the disconnected status.
    fn set_disconnected(&mut self, value: bool);

    /// Initializes a new instance of the Connection.
    fn new(stream: TcpStream, remote: SocketAddr, local: SocketAddr) -> Self where Self: Sized;

    /// Disconnect from the remote node.
    async fn disconnect(&mut self, abort: bool) {
        self.set_disconnected(true);
        if abort {
            self.stream_mut().abort().await.ok();
        } else {
            self.stream_mut().shutdown().await.ok();
        }
    }

    /// Called when a TCP ACK message is received.
    async fn on_ack(&mut self) {
        // Implementation depends on specific requirements
    }

    /// Called when data is received.
    async fn on_data(&mut self, data: BytesMut);

    /// Main loop for handling incoming messages
    async fn run(&mut self) {
        let mut buf = BytesMut::with_capacity(1024);

        loop {
            match timeout(Duration::from_secs(Self::CONNECTION_TIMEOUT_LIMIT), self.stream_mut().read_buf(&mut buf)).await {
                Ok(Ok(0)) => {
                    // Connection closed
                    break;
                }
                Ok(Ok(_)) => {
                    // Data received
                    if self.on_data(buf.split()).await.is_err() {
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
    async fn send_data(&mut self, data: Bytes) -> std::io::Result<()> {
        self.stream_mut().write_all(&data).await
    }
}