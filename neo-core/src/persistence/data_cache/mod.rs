pub use neo_storage::persistence::data_cache::PrefetchPattern;
pub use neo_storage::persistence::data_cache::cache::*;
pub use neo_storage::persistence::data_cache::trackable::*;

#[cfg(feature = "runtime")]
mod storage_watch;

#[cfg(feature = "runtime")]
pub(crate) use storage_watch::{
    StorageWatchPhase, clear_storage_watch_context, set_storage_watch_context,
};
