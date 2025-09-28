//! P2P connection management and state.
//!
//! This module implements connection management exactly matching C# Neo's Peer and Connection classes.

use super::message::PAYLOAD_MAX_SIZE;
use crate::network::{
    error::{NetworkError, NetworkResult},
    p2p::messages::NetworkMessage,
};
use std::net::SocketAddr;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};

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
    pub fn new(stream: TcpStream, address: SocketAddr, inbound: bool) -> Self {
        let (message_tx, _) = mpsc::unbounded_channel();
        let now = std::time::Instant::now();

        Self {
            state: ConnectionState::Connected,
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
    pub async fn send_message(&mut self, message: NetworkMessage) -> NetworkResult<()> {
        if !self.state.is_active() {
            return Err(NetworkError::ConnectionError(
                "Connection not active".to_string(),
            ));
        }

        let command = message.command();
        let bytes = message.to_bytes(self.compression_allowed)?;

        tracing::debug!(
            "Sending message to {}: {} bytes, command: {:?}",
            self.address,
            bytes.len(),
            command
        );
        tracing::debug!("First bytes: {:02x?}", &bytes[..24.min(bytes.len())]);

        // Send bytes over TCP stream
        self.stream
            .write_all(&bytes)
            .await
            .map_err(|e| NetworkError::ConnectionError(format!("Failed to send message: {}", e)))?;

        // Update activity timestamp
        self.update_activity();

        Ok(())
    }

    /// Receives a message from the peer (matches C# RemoteNode.ReceiveMessage exactly)
    pub async fn receive_message(&mut self) -> NetworkResult<NetworkMessage> {
        if !self.state.is_active() {
            return Err(NetworkError::ConnectionError(
                "Connection not active".to_string(),
            ));
        }

        let mut message_bytes = Vec::new();

        let flag_byte = self.read_exact_byte().await?;
        message_bytes.push(flag_byte);

        let command_byte = self.read_exact_byte().await?;
        message_bytes.push(command_byte);

        let (payload_length, mut length_bytes) = Self::read_var_int_async(&mut self.stream).await?;
        message_bytes.append(&mut length_bytes);

        let mut payload = vec![0u8; payload_length as usize];
        if payload_length > 0 {
            self.stream.read_exact(&mut payload).await.map_err(|e| {
                NetworkError::ConnectionError(format!("Failed to read payload: {}", e))
            })?;
        }
        message_bytes.extend_from_slice(&payload);

        let message = NetworkMessage::from_bytes(&message_bytes)?;

        self.update_activity();

        Ok(message)
    }

    async fn read_exact_byte(&mut self) -> NetworkResult<u8> {
        let mut buffer = [0u8; 1];
        self.stream
            .read_exact(&mut buffer)
            .await
            .map_err(|e| NetworkError::ConnectionError(format!("Failed to read byte: {}", e)))?;
        Ok(buffer[0])
    }

    async fn read_var_int_async(stream: &mut TcpStream) -> NetworkResult<(u64, Vec<u8>)> {
        let mut first = [0u8; 1];
        stream.read_exact(&mut first).await.map_err(|e| {
            NetworkError::ConnectionError(format!("Failed to read varint prefix: {}", e))
        })?;

        let mut bytes = vec![first[0]];
        let value = match first[0] {
            0xFD => {
                let mut buffer = [0u8; 2];
                stream.read_exact(&mut buffer).await.map_err(|e| {
                    NetworkError::ConnectionError(format!("Failed to read varint (u16): {}", e))
                })?;
                bytes.extend_from_slice(&buffer);
                u16::from_le_bytes(buffer) as u64
            }
            0xFE => {
                let mut buffer = [0u8; 4];
                stream.read_exact(&mut buffer).await.map_err(|e| {
                    NetworkError::ConnectionError(format!("Failed to read varint (u32): {}", e))
                })?;
                bytes.extend_from_slice(&buffer);
                u32::from_le_bytes(buffer) as u64
            }
            0xFF => {
                let mut buffer = [0u8; 8];
                stream.read_exact(&mut buffer).await.map_err(|e| {
                    NetworkError::ConnectionError(format!("Failed to read varint (u64): {}", e))
                })?;
                bytes.extend_from_slice(&buffer);
                u64::from_le_bytes(buffer)
            }
            value => value as u64,
        };

        if value > PAYLOAD_MAX_SIZE as u64 {
            return Err(NetworkError::InvalidMessage(format!(
                "Payload length {} exceeds maximum {}",
                value, PAYLOAD_MAX_SIZE
            )));
        }

        Ok((value, bytes))
    }

    /// Closes the connection gracefully
    pub async fn close(&mut self) -> NetworkResult<()> {
        self.state = ConnectionState::Disconnecting;

        // Shutdown the TCP stream
        if let Err(e) = self.stream.shutdown().await {
            tracing::warn!("Failed to shutdown connection to {}: {}", self.address, e);
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
mod tests {}
