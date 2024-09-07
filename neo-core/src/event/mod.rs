pub mod icommitted_handler;
pub mod icommitting_handler;
pub mod ilog_handler;
pub mod ilogging_handler;
pub mod imessage_received_handler;
pub mod inotify_handler;
pub mod iservice_added_handler;
pub mod itransaction_added_handler;
pub mod itransaction_removed_handler;
pub mod iwallet_changed_handler;


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
