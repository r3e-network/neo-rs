use crate::{
	item_trait::{StackItemTrait},
};
use std::{
	cell::{Ref, RefCell},
	hash::Hash,
};
use crate::stack_item::StackItem;

pub trait CompoundTrait: StackItemTrait {
	fn count(&self) -> usize;
	fn sub_items(&self) -> Vec<Ref<RefCell<StackItem>>>;
	fn sub_items_count(&self) -> usize{
		self.sub_items().len()
	}
	fn read_only(&self);
	fn is_read_only(&self) -> bool {
		false
	}

	fn clear(&mut self);

	fn as_bool(&self) -> bool {
		true
	}
}