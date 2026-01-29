use neo_core::UInt160;

/// Represents an address for JSON-RPC parameters. Matches the semantics of the
/// C# `Neo.Plugins.RpcServer.Model.Address` record by storing the script hash
/// together with the address version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    script_hash: UInt160,
    address_version: u8,
}

impl Address {
    #[must_use] 
    pub const fn new(script_hash: UInt160, address_version: u8) -> Self {
        Self {
            script_hash,
            address_version,
        }
    }

    #[must_use] 
    pub const fn script_hash(&self) -> &UInt160 {
        &self.script_hash
    }

    #[must_use] 
    pub const fn address_version(&self) -> u8 {
        self.address_version
    }
}
