use std::{
	hash::Hash,
};

use crate::stack_item::{SharedItem};

pub trait CompoundTrait {
	fn ref_count(&self) -> usize;
	fn ref_inc(&mut self, count:usize) -> usize;
	fn ref_dec(&mut self, count:usize) -> usize;
	fn sub_items(&self) -> Vec<SharedItem>;
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