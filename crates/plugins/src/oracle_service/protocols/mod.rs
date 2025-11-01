//! Oracle Service Protocols
//!
//! Protocol implementations for Oracle Service.

pub mod i_oracle_protocol;
pub mod oracle_https_protocol;
pub mod oracle_neo_fs_protocol;

pub use i_oracle_protocol::IOracleProtocol;
pub use oracle_https_protocol::OracleHttpsProtocol;
pub use oracle_neo_fs_protocol::OracleNeoFSProtocol;
