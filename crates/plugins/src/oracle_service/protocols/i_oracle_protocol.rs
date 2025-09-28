//! Oracle Protocol Interface
//!
//! Interface for Oracle protocols.

/// Oracle Protocol Interface
pub trait IOracleProtocol {
    /// Execute the oracle protocol
    fn execute(&self) -> Result<String, String>;
}
