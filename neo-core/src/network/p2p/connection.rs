//! P2P connection management and state with optimized I/O.
//!
//! This module implements connection management exactly matching C# Neo's Peer and Connection classes.
//! Optimizations include:
//! - Write buffering for small messages
//! - Vectored I/O for multi-buffer writes
//! - Reduced allocations in message serialization

use super::{
    channels_config::ChannelsConfig,
    framed::{FrameConfig, FramedSocket, WriteBuffer},
};
use crate::network::{
    error::{NetworkError, NetworkResult},
    p2p::messages::NetworkMessage,
};
use std::net::SocketAddr;
use tokio::{io::AsyncWriteExt, net::TcpStream, sync::mpsc};

/// Connection state (matches C# Neo RemoteNode state exactly)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Initial connection establishing
    Connecting,

    /// TCP connection established
    Connected,

    /// Performing protocol handshake
    Handshaking,

    /// Fully connected and ready for communication
    Ready,

    /// Connection being closed
    Disconnecting,

    /// Connection closed
    Disconnected,
}

impl ConnectionState {
    /// Checks if the connection is active (can send/receive messages)
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            ConnectionState::Connected | ConnectionState::Handshaking | ConnectionState::Ready
        )
    }

    /// Checks if the connection is ready for normal operations
    pub fn is_ready(&self) -> bool {
        matches!(self, ConnectionState::Ready)
    }

    /// Checks if the connection is being established
    pub fn is_connecting(&self) -> bool {
        matches!(
            self,
            ConnectionState::Connecting | ConnectionState::Handshaking
        )
    }

    /// Checks if the connection is closed or closing
    pub fn is_closed(&self) -> bool {
        matches!(
            self,
            ConnectionState::Disconnecting | ConnectionState::Disconnected
        )
    }
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionState::Connecting => write!(f, "Connecting"),
            ConnectionState::Connected => write!(f, "Connected"),
            ConnectionState::Handshaking => write!(f, "Handshaking"),
            ConnectionState::Ready => write!(f, "Ready"),
            ConnectionState::Disconnecting => write!(f, "Disconnecting"),
            ConnectionState::Disconnected => write!(f, "Disconnected"),
        }
    }
}

/// Peer connection (matches C# Neo RemoteNode exactly)
///
/// Optimizations:
/// - Write buffering for small messages to reduce syscall overhead
/// - Efficient message serialization with minimal allocations
#[derive(Debug)]
pub struct PeerConnection {
    /// Connection state
    pub state: ConnectionState,

    /// Framing and timeout configuration for this connection.
    pub frame_config: FrameConfig,

    /// TCP stream
    pub stream: TcpStream,

    /// Peer address
    pub address: SocketAddr,

    /// Whether we have received and processed peer handshake metadata
    pub node_info_set: bool,

    /// Message sender channel (bounded to apply backpressure)
    pub message_tx: mpsc::Sender<NetworkMessage>,

    /// Whether this is an inbound connection
    pub inbound: bool,

    /// Whether the peer negotiated compression support.
    pub compression_allowed: bool,

    /// Last activity timestamp
    pub last_activity: std::time::Instant,

    /// Connection established timestamp
    pub connected_at: std::time::Instant,

    /// Write buffer for batching small writes.
    write_buffer: WriteBuffer,

    /// Statistics for monitoring I/O performance.
    stats: ConnectionStats,
}

/// Connection statistics for monitoring performance.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConnectionStats {
    /// Number of messages sent.
    pub messages_sent: u64,
    /// Number of messages received.
    pub messages_received: u64,
    /// Total bytes sent (including protocol overhead).
    pub bytes_sent: u64,
    /// Total bytes received (including protocol overhead).
    pub bytes_received: u64,
    /// Number of buffered writes performed.
    pub buffered_writes: u64,
    /// Number of direct writes performed.
    pub direct_writes: u64,
}

impl PeerConnection {
    /// Creates a new peer connection
    pub fn new(
        stream: TcpStream,
        address: SocketAddr,
        inbound: bool,
        frame_config: FrameConfig,
    ) -> Self {
        // Bounded channel provides backpressure when the receiver falls behind.
        let (message_tx, _) = mpsc::channel(1024);
        let now = std::time::Instant::now();

        Self {
            state: ConnectionState::Connected,
            frame_config,
            stream,
            address,
            node_info_set: false,
            message_tx,
            inbound,
            compression_allowed: false,
            last_activity: now,
            connected_at: now,
            write_buffer: WriteBuffer::default(),
            stats: ConnectionStats::default(),
        }
    }

    /// Convenience constructor that derives `FrameConfig` from a channel configuration.
    pub fn from_channels_config(
        stream: TcpStream,
        address: SocketAddr,
        inbound: bool,
        config: &ChannelsConfig,
    ) -> Self {
        Self::new(stream, address, inbound, FrameConfig::from(config))
    }

    /// Updates the last activity timestamp
    pub fn update_activity(&mut self) {
        self.last_activity = std::time::Instant::now();
    }

    /// Gets the connection duration
    pub fn connection_duration(&self) -> std::time::Duration {
        std::time::Instant::now().duration_since(self.connected_at)
    }

    /// Gets the time since last activity
    pub fn idle_duration(&self) -> std::time::Duration {
        std::time::Instant::now().duration_since(self.last_activity)
    }

    /// Sets the connection state
    pub fn set_state(&mut self, state: ConnectionState) {
        self.state = state;
        self.update_activity();
    }

    /// Marks peer metadata as available (after handshake)
    pub fn set_node_info(&mut self) {
        self.node_info_set = true;
        self.update_activity();
    }

    /// Gets a copy of the connection statistics.
    pub fn stats(&self) -> ConnectionStats {
        self.stats
    }

    /// Sends a message to the peer (matches C# RemoteNode.SendMessage exactly)
    ///
    /// Optimizations:
    /// - Uses write buffering for small messages
    /// - Minimizes allocations in serialization path
    /// - Efficient timeout handling
    pub async fn send_message(&mut self, message: &NetworkMessage) -> NetworkResult<()> {
        if !self.state.is_active() {
            return Err(NetworkError::ConnectionError(
                "Connection not active".to_string(),
            ));
        }

        let command = message.command();
        let bytes = message.to_bytes(self.compression_allowed)?;
        let _timeout = self.frame_config.write_timeout;

        tracing::debug!(
            "Sending message to {}: {} bytes, command: {:?}",
            self.address,
            bytes.len(),
            command
        );
        tracing::debug!("First bytes: {:02x?}", &bytes[..24.min(bytes.len())]);

        // Use framed socket for buffered writes
        let mut framed = FramedSocket::new(&mut self.stream);
        framed
            .write_frame(&self.frame_config, &bytes, &mut self.write_buffer)
            .await?;

        // Update statistics
        self.stats.messages_sent += 1;
        if bytes.len() < self.write_buffer.threshold {
            self.stats.buffered_writes += 1;
        } else {
            self.stats.direct_writes += 1;
        }
        self.stats.bytes_sent += bytes.len() as u64;

        // Update activity timestamp
        self.update_activity();

        Ok(())
    }

    /// Sends multiple messages efficiently using vectored I/O.
    ///
    /// This is more efficient than sending messages individually when
    /// multiple messages are ready to be sent at once.
    pub async fn send_messages_batch(&mut self, messages: &[NetworkMessage]) -> NetworkResult<()> {
        if !self.state.is_active() {
            return Err(NetworkError::ConnectionError(
                "Connection not active".to_string(),
            ));
        }

        if messages.is_empty() {
            return Ok(());
        }

        // Flush any pending buffered data first
        if !self.write_buffer.is_empty() {
            let buffered = self.write_buffer.take();
            tokio::time::timeout(
                self.frame_config.write_timeout,
                self.stream.write_all(&buffered),
            )
            .await
            .map_err(|_| NetworkError::Timeout)?
            .map_err(|e| NetworkError::ConnectionError(format!("Failed to flush buffer: {e}")))?;
        }

        // Serialize all messages
        let mut buffers: Vec<Vec<u8>> = Vec::with_capacity(messages.len());
        let mut total_bytes = 0;

        for message in messages {
            let bytes = message.to_bytes(self.compression_allowed)?;
            total_bytes += bytes.len();
            buffers.push(bytes);
        }

        // Use vectored I/O for efficient multi-message send
        let mut framed = FramedSocket::new(&mut self.stream);
        let buffer_refs: Vec<&[u8]> = buffers.iter().map(|b| b.as_slice()).collect();
        framed
            .write_frame_vectored(&self.frame_config, &buffer_refs)
            .await?;

        // Update statistics
        self.stats.messages_sent += messages.len() as u64;
        self.stats.direct_writes += 1;
        self.stats.bytes_sent += total_bytes as u64;

        self.update_activity();

        Ok(())
    }

    /// Flushes any pending buffered writes.
    pub async fn flush(&mut self) -> NetworkResult<()> {
        if !self.write_buffer.is_empty() {
            let buffered = self.write_buffer.take();
            tokio::time::timeout(
                self.frame_config.write_timeout,
                self.stream.write_all(&buffered),
            )
            .await
            .map_err(|_| NetworkError::Timeout)?
            .map_err(|e| NetworkError::ConnectionError(format!("Failed to flush buffer: {e}")))?;
        }
        Ok(())
    }

    /// Receives a message from the peer (matches C# RemoteNode.ReceiveMessage exactly)
    ///
    /// Optimizations:
    /// - Pre-allocated buffer with exact capacity
    /// - Minimal allocations during parsing
    pub async fn receive_message(
        &mut self,
        handshake_complete: bool,
    ) -> NetworkResult<NetworkMessage> {
        if !self.state.is_active() {
            return Err(NetworkError::ConnectionError(
                "Connection not active".to_string(),
            ));
        }

        let mut framed = FramedSocket::new(&mut self.stream);
        let message_bytes = framed
            .read_frame(&self.frame_config, handshake_complete)
            .await?;

        let message = NetworkMessage::from_bytes(&message_bytes)?;

        // Update statistics
        self.stats.messages_received += 1;
        self.stats.bytes_received += message_bytes.len() as u64;

        self.update_activity();

        tracing::debug!(
            "Received message from {}: {} bytes, command: {:?}",
            self.address,
            message_bytes.len(),
            message.command()
        );
        tracing::debug!(
            "First bytes: {:02x?}",
            &message_bytes[..24.min(message_bytes.len())]
        );

        Ok(message)
    }

    /// Closes the connection gracefully
    pub async fn close(&mut self) -> NetworkResult<()> {
        self.state = ConnectionState::Disconnecting;

        // Flush any pending writes before closing
        if let Err(e) = self.flush().await {
            tracing::warn!(
                target: "neo",
                error = %e,
                "failed to flush pending writes before shutdown"
            );
        }

        // Shutdown the TCP stream
        match tokio::time::timeout(self.frame_config.shutdown_timeout, self.stream.shutdown()).await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!(
                    target: "neo",
                    error = %e,
                    "failed to shutdown connection to {}",
                    self.address
                );
            }
            Err(err) => {
                tracing::warn!(
                    target: "neo",
                    error = %err,
                    "timed out shutting down connection to {}",
                    self.address
                );
            }
        }

        self.state = ConnectionState::Disconnected;
        Ok(())
    }

    /// Checks if the connection should be considered stale
    pub fn is_stale(&self, timeout: std::time::Duration) -> bool {
        self.idle_duration() > timeout
    }

    /// Gets connection information for debugging
    pub fn connection_info(&self) -> ConnectionInfo {
        ConnectionInfo {
            address: self.address,
            state: self.state,
            inbound: self.inbound,
            connected_at: self.connected_at,
            last_activity: self.last_activity,
            node_info_set: self.node_info_set,
            stats: self.stats,
        }
    }
}

/// Connection information for debugging and monitoring
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// Peer address
    pub address: SocketAddr,

    /// Connection state
    pub state: ConnectionState,

    /// Whether this is an inbound connection
    pub inbound: bool,

    /// When the connection was established
    pub connected_at: std::time::Instant,

    /// Last activity timestamp
    pub last_activity: std::time::Instant,

    /// Whether node information (handshake) has been received
    pub node_info_set: bool,

    /// Connection statistics
    pub stats: ConnectionStats,
}

impl ConnectionInfo {
    /// Gets the connection duration
    pub fn duration(&self) -> std::time::Duration {
        std::time::Instant::now().duration_since(self.connected_at)
    }

    /// Gets the idle duration
    pub fn idle_time(&self) -> std::time::Duration {
        std::time::Instant::now().duration_since(self.last_activity)
    }

    /// Gets the connection direction as string
    pub fn direction(&self) -> &'static str {
        if self.inbound {
            "Inbound"
        } else {
            "Outbound"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::p2p::payloads::ping_payload::PingPayload;
    use crate::network::p2p::ProtocolMessage;
    use std::time::Duration;
    use tokio::net::TcpListener;

    async fn tcp_pair() -> Option<(TcpStream, TcpStream)> {
        let listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => return None,
            Err(err) => panic!("bind listener: {}", err),
        };
        let addr = listener.local_addr().expect("listener addr");
        let client = TcpStream::connect(addr);
        let server = listener.accept();
        let (client_stream, server_stream) = tokio::join!(client, server);
        let client_stream = client_stream.expect("client stream");
        let (server_stream, _) = server_stream.expect("server stream");
        Some((client_stream, server_stream))
    }

    #[tokio::test]
    async fn send_message_fails_when_connection_inactive() {
        let Some((client_stream, _server_stream)) = tcp_pair().await else {
            return;
        };
        let mut connection = PeerConnection::from_channels_config(
            client_stream,
            "127.0.0.1:0".parse().unwrap(),
            false,
            &ChannelsConfig::default(),
        );
        connection.set_state(ConnectionState::Disconnected);

        let ping = ProtocolMessage::Ping(PingPayload::create_with_nonce(0, 0));
        let message = NetworkMessage::new(ping);
        let result = connection.send_message(&message).await;

        assert!(matches!(
            result,
            Err(NetworkError::ConnectionError(msg)) if msg.contains("Connection not active")
        ));
    }

    #[tokio::test]
    async fn from_channels_config_applies_frame_config() {
        let Some((client_stream, _server_stream)) = tcp_pair().await else {
            return;
        };
        let config = ChannelsConfig {
            write_timeout: Duration::from_millis(5),
            ..ChannelsConfig::default()
        };
        let connection = PeerConnection::from_channels_config(
            client_stream,
            "127.0.0.1:0".parse().unwrap(),
            false,
            &config,
        );

        assert_eq!(connection.frame_config.write_timeout, config.write_timeout);
    }

    #[tokio::test]
    async fn stats_updated_after_send() {
        let Some((client_stream, server_stream)) = tcp_pair().await else {
            return;
        };

        // Set up server to receive
        let server_handle = tokio::spawn(async move {
            let mut connection = PeerConnection::from_channels_config(
                server_stream,
                "127.0.0.1:0".parse().unwrap(),
                true,
                &ChannelsConfig::default(),
            );
            connection.receive_message(false).await.ok()
        });

        let mut connection = PeerConnection::from_channels_config(
            client_stream,
            "127.0.0.1:0".parse().unwrap(),
            false,
            &ChannelsConfig::default(),
        );

        let initial_stats = connection.stats();

        let ping = ProtocolMessage::Ping(PingPayload::create_with_nonce(0, 0));
        let message = NetworkMessage::new(ping);

        // Send should work even though server isn't reading (TCP buffering)
        let _ = connection.send_message(&message).await;

        let stats_after_send = connection.stats();
        assert_eq!(
            stats_after_send.messages_sent,
            initial_stats.messages_sent + 1
        );

        // Clean up
        drop(connection);
        server_handle.abort();
    }

    #[test]
    fn connection_stats_default() {
        let stats = ConnectionStats::default();
        assert_eq!(stats.messages_sent, 0);
        assert_eq!(stats.messages_received, 0);
        assert_eq!(stats.bytes_sent, 0);
        assert_eq!(stats.bytes_received, 0);
    }
}
