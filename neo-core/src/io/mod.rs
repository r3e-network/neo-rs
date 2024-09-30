pub mod caching;
pub mod serializable_trait;
pub mod memory_reader;
pub mod byte_array_comparer;
pub mod byte_array_equality_comparer;
pub mod binary_writer;
pub mod binary_reader;
mod io_error;
mod priority_mailbox;
mod priority_message_queue;

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
