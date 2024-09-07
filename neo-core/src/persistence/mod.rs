mod cloned_cache;
mod data_cache;
mod iread_only_store;
mod isnapshot;
mod istore;
mod istore_provider;
mod memory_snapshot;
mod memory_store;
mod memory_store_provider;
mod seek_direction;
mod snapshot_cache;
mod store_factory;
mod track_state;

pub use cloned_cache::*;
pub use data_cache::*;
pub use iread_only_store::*;
pub use isnapshot::*;
pub use istore::*;
pub use istore_provider::*;
pub use memory_snapshot::*;
pub use memory_store::*;
pub use memory_store_provider::*;
pub use seek_direction::*;
pub use snapshot_cache::*;
pub use store_factory::*;
pub use track_state::*;


pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
