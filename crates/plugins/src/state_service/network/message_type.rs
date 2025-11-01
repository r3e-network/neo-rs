// Copyright (C) 2015-2025 The Neo Project.
//
// message_type.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// Message type enumeration for state service network messages.
/// Matches C# MessageType enum exactly
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MessageType {
    /// Vote message type
    /// Matches C# Vote variant
    Vote = 0,
    
    /// State root message type
    /// Matches C# StateRoot variant
    StateRoot = 1,
}

impl MessageType {
    /// Converts a byte to MessageType.
    /// Matches C# implicit conversion behavior
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(MessageType::Vote),
            1 => Some(MessageType::StateRoot),
            _ => None,
        }
    }
    
    /// Converts MessageType to byte.
    /// Matches C# implicit conversion behavior
    pub fn to_byte(self) -> u8 {
        self as u8
    }
}

impl From<u8> for MessageType {
    fn from(byte: u8) -> Self {
        Self::from_byte(byte).unwrap_or_else(|| {
            panic!("Invalid MessageType byte: {}", byte)
        })
    }
}

impl From<MessageType> for u8 {
    fn from(message_type: MessageType) -> Self {
        message_type.to_byte()
    }
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageType::Vote => write!(f, "Vote"),
            MessageType::StateRoot => write!(f, "StateRoot"),
        }
    }
}