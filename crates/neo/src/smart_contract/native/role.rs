//! Role - matches C# Neo.SmartContract.Native.Role exactly

/// Represents roles in the Neo network (matches C# Role enum)
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// State validator role
    StateValidator = 4,
    
    /// Oracle role
    Oracle = 8,
    
    /// NeoFS Alphabet Node role
    NeoFSAlphabetNode = 16,
    
    /// P2P Notary role (for NotaryAssisted transactions attribute)
    P2PNotary = 32,
}

impl Role {
    /// Creates from byte value
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            4 => Some(Role::StateValidator),
            8 => Some(Role::Oracle),
            16 => Some(Role::NeoFSAlphabetNode),
            32 => Some(Role::P2PNotary),
            _ => None,
        }
    }
    
    /// Checks if the role is valid
    pub fn is_valid(&self) -> bool {
        matches!(self, 
            Role::StateValidator | 
            Role::Oracle | 
            Role::NeoFSAlphabetNode | 
            Role::P2PNotary
        )
    }
    
    /// Gets the name of the role
    pub fn name(&self) -> &'static str {
        match self {
            Role::StateValidator => "StateValidator",
            Role::Oracle => "Oracle",
            Role::NeoFSAlphabetNode => "NeoFSAlphabetNode",
            Role::P2PNotary => "P2PNotary",
        }
    }
}