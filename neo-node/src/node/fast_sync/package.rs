//! Official fast-sync package acquisition facade.

mod cache;
mod extract;
mod manifest;

pub(super) use cache::ensure_package_cached;
pub(super) use extract::ensure_chain_acc_extracted;
pub(super) use manifest::fetch_latest_package;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FastSyncPackage {
    pub(super) network_key: &'static str,
    pub(super) url: String,
    pub(super) md5: String,
    pub(super) start: u32,
    pub(super) end: u32,
    pub(super) filename: String,
}
