use neo_primitives::UInt160;

/// Represents an address for JSON-RPC parameters. Matches the semantics of the
/// C# `Neo.Plugins.RpcServer.Model.Address` record by storing the script hash
/// together with the address version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    script_hash: UInt160,
    address_version: u8,
}

impl Address {
    /// Create an address parameter from a script hash and address version.
    #[must_use]
    pub const fn new(script_hash: UInt160, address_version: u8) -> Self {
        Self {
            script_hash,
            address_version,
        }
    }

    /// Return the script hash carried by this address.
    #[must_use]
    pub const fn script_hash(&self) -> &UInt160 {
        &self.script_hash
    }

    /// Return the address version byte used to encode this address.
    #[must_use]
    pub const fn address_version(&self) -> u8 {
        self.address_version
    }
}
