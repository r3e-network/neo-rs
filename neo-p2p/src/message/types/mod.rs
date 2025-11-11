mod alerts;
mod blocks;
mod endpoint;
mod filters;
mod handshake;
mod inventory;

pub use alerts::{AlertPayload, RejectPayload};
pub use blocks::{GetBlockByIndexPayload, GetBlocksPayload, HeadersPayload, MerkleBlockPayload};
pub use endpoint::{AddressEntry, AddressPayload, Endpoint, NetworkAddress};
pub use filters::{FilterAddPayload, FilterLoadPayload};
pub use handshake::{PingPayload, VersionPayload};
pub use inventory::{InventoryItem, InventoryKind, InventoryPayload, PayloadWithData};

pub const MAX_INVENTORY_ITEMS: u64 = 4096;
pub const MAX_ADDRESS_COUNT: u64 = 1024;
pub const MAX_HEADERS_COUNT: u64 = 2000;
