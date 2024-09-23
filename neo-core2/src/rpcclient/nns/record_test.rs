use std::error::Error;
use neo_core2::vm::stackitem::StackItem;
use neo_core2::vm::stackitem::Item;
use neo_core2::rpcclient::nns::RecordState;
use anyhow::Result;

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn test_record_state_from_stack_item() -> Result<()> {
        let r = RecordState::default();
        assert!(r.from_stack_item(StackItem::from(42)).is_err());
        assert!(r.from_stack_item(StackItem::from(Vec::<Item>::new())).is_err());
        assert!(r.from_stack_item(StackItem::from(vec![
            StackItem::from(Vec::<Item>::new()),
            StackItem::from(16),
            StackItem::from("cool"),
        ])).is_err());
        assert!(r.from_stack_item(StackItem::from(vec![
            StackItem::from("n3"),
            StackItem::from(Vec::<Item>::new()),
            StackItem::from("cool"),
        ])).is_err());
        assert!(r.from_stack_item(StackItem::from(vec![
            StackItem::from("n3"),
            StackItem::from(16),
            StackItem::from(Vec::<Item>::new()),
        ])).is_err());
        assert!(r.from_stack_item(StackItem::from(vec![
            StackItem::from("n3"),
            StackItem::from(100500),
            StackItem::from("cool"),
        ])).is_err());
        assert!(r.from_stack_item(StackItem::from(vec![
            StackItem::from("n3"),
            StackItem::from(16),
            StackItem::from("cool"),
        ])).is_ok());
        Ok(())
    }
}
