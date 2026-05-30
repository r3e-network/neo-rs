//! P2P connection lifecycle state.

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
