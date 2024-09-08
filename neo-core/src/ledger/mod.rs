pub mod blockchain;
pub mod header_cache;
pub mod memory_pool;
pub mod pool_item;
pub mod transaction_removal_reason;
pub mod transaction_removed_event_args;
pub mod transaction_router;
pub mod transaction_verification_context;
pub mod verify_result;
pub mod blockchain_application_executed;

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
