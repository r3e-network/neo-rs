//! P2P connection management and state.
//!
//! This module implements connection management exactly matching C# Neo's Peer and Connection classes.

use super::{
    channels_config::ChannelsConfig,
    framed::{FrameConfig, FramedSocket},
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

    /// Message sender channel
    pub message_tx: mpsc::UnboundedSender<NetworkMessage>,

    /// Whether this is an inbound connection
    pub inbound: bool,

    /// Whether the peer negotiated compression support.
    pub compression_allowed: bool,

    /// Last activity timestamp
    pub last_activity: std::time::Instant,

    /// Connection established timestamp
    pub connected_at: std::time::Instant,
}

impl PeerConnection {
    /// Creates a new peer connection
    pub fn new(
        stream: TcpStream,
        address: SocketAddr,
        inbound: bool,
        frame_config: FrameConfig,
    ) -> Self {
        let (message_tx, _) = mpsc::unbounded_channel();
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

    /// Sends a message to the peer (matches C# RemoteNode.SendMessage exactly)
    pub async fn send_message(&mut self, message: &NetworkMessage) -> NetworkResult<()> {
        if !self.state.is_active() {
            return Err(NetworkError::ConnectionError(
                "Connection not active".to_string(),
            ));
        }

        let command = message.command();
        let bytes = message.to_bytes(self.compression_allowed)?;
        let timeout = self.frame_config.write_timeout;

        tracing::debug!(
            "Sending message to {}: {} bytes, command: {:?}",
            self.address,
            bytes.len(),
            command
        );
        tracing::debug!("First bytes: {:02x?}", &bytes[..24.min(bytes.len())]);

        // Send bytes over TCP stream
        tokio::time::timeout(timeout, self.stream.write_all(&bytes))
            .await
            .map_err(|_| NetworkError::Timeout)?
            .map_err(|e| NetworkError::ConnectionError(format!("Failed to send message: {}", e)))?;

        // Update activity timestamp
        self.update_activity();

        Ok(())
    }

    /// Receives a message from the peer (matches C# RemoteNode.ReceiveMessage exactly)
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

    async fn tcp_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let client = TcpStream::connect(addr);
        let server = listener.accept();
        let (client_stream, server_stream) = tokio::join!(client, server);
        let client_stream = client_stream.expect("client stream");
        let (server_stream, _) = server_stream.expect("server stream");
        (client_stream, server_stream)
    }

    #[tokio::test]
    async fn send_message_fails_when_connection_inactive() {
        let (client_stream, _server_stream) = tcp_pair().await;
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
        let (client_stream, _server_stream) = tcp_pair().await;
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
}
