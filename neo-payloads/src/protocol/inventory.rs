//! Re-export of the Inventory trait from neo-primitives.
//!
//! The canonical `Inventory` trait now lives in [`neo_primitives`] so that
//! both neo-core (implementations) and neo-p2p (networking) can depend on
//! it without a circular dependency.

pub use neo_primitives::Inventory;
pub use neo_primitives::InventoryType;
