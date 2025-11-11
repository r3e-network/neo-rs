mod entry;
mod network;
mod payload;
mod types;

pub use entry::AddressEntry;
pub use network::NetworkAddress;
pub use payload::AddressPayload;
pub use types::Endpoint;

pub(super) const MAX_ADDRESSES: u64 = super::MAX_ADDRESS_COUNT;
