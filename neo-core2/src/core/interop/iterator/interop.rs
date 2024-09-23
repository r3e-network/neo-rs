use crate::core::interop;
use crate::vm::stackitem::{self, StackItem};

pub trait Iterator {
    fn next(&mut self) -> bool;
    fn value(&self) -> StackItem;
}

// Next advances the iterator, pushes true on success and false otherwise.
pub fn next(ic: &mut interop::Context) -> Result<(), String> {
    let iop = ic.vm.estack().pop().interop();
    let mut arr = iop.value().downcast_mut::<dyn Iterator>().ok_or("Invalid iterator")?;
    ic.vm.estack().push_item(StackItem::Bool(arr.next()));

    Ok(())
}

// Value returns current iterator value and depends on iterator type:
// For slices the result is just value.
// For maps the result is key-value pair packed in a struct.
pub fn value(ic: &mut interop::Context) -> Result<(), String> {
    let iop = ic.vm.estack().pop().interop();
    let arr = iop.value().downcast_ref::<dyn Iterator>().ok_or("Invalid iterator")?;
    ic.vm.estack().push_item(arr.value());

    Ok(())
}

// IsIterator returns whether stackitem implements iterator interface.
pub fn is_iterator(item: &StackItem) -> bool {
    item.value().is::<dyn Iterator>()
}

// ValuesTruncated returns an array of up to `max_num` iterator values. The second
// return parameter denotes whether iterator is truncated, i.e. has more values.
// The provided iterator CAN NOT be reused in the subsequent calls to Values and
// to ValuesTruncated.
pub fn values_truncated(item: &StackItem, max_num: usize) -> (Vec<StackItem>, bool) {
    let result = values(item, max_num);
    let mut arr = item.value().downcast_ref::<dyn Iterator>().unwrap();
    (result, arr.next())
}

// Values returns an array of up to `max_num` iterator values. The provided
// iterator can safely be reused to retrieve the rest of its values in the
// subsequent calls to Values and to ValuesTruncated.
pub fn values(item: &StackItem, max_num: usize) -> Vec<StackItem> {
    let mut result = Vec::new();
    let mut arr = item.value().downcast_ref::<dyn Iterator>().unwrap();
    let mut count = max_num;
    while count > 0 && arr.next() {
        result.push(arr.value());
        count -= 1;
    }
    result
}
