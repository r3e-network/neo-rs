//! StorageItem - matches C# Neo.SmartContract.StorageItem.
//!
//! The canonical definition lives in `neo-storage` (a leaf storage crate); this
//! module simply re-exports it. The cache-aware extension trait that needs
//! smart-contract VM interop lives in `crate::smart_contract::storage_item_ext`
//! so the persistence/storage layer carries no edge into the smart-contract layer.

pub use neo_storage::StorageItem;
