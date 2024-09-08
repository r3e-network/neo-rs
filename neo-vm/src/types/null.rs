use crate::{
	item_trait::{ObjectReferenceEntry, StackItemTrait},
	item_type::ItemType,
};
use std::{
	cell::RefCell,
	collections::HashMap,
	fmt::{Debug, Formatter},
	hash::{Hash, Hasher},
};
use num_bigint::BigInt;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use crate::compound_types::compound_trait::CompoundTrait;
use crate::execution_engine_limits::ExecutionEngineLimits;

/// Represents `null` in the vm.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Null {
}

impl PartialEq<Self> for Null {
	fn eq(&self, other: &Self) -> bool {
		todo!()
	}
}

impl Serialize for Null {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer
	{
		todo!()
	}
}

impl Deserialize for Null {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>
	{
		todo!()
	}
}

impl StackItemTrait for Null {
	type Item = ();
	type ItemType = ();
	type ObjectReferences = ();
	type DFN = ();
	type LowLink = ();
	type OnStack = ();

	fn dfn(&self) -> isize {
		self.dfn
	}

	fn set_dfn(&mut self, dfn: isize) {
		self.dfn = dfn;
	}

	fn low_link(&self) -> usize {
		self.low_link
	}

	fn set_low_link(&mut self, link: usize) {
		self.low_link = link;
	}

	fn on_stack(&self) -> bool {
		self.on_stack
	}

	fn set_on_stack(&mut self, on_stack: bool) {
		self.on_stack = on_stack;
	}

	fn set_object_references(&mut self, refs: Self::ObjectReferences) {
		self.object_references = refs;
	}

	fn object_references(&self) -> &Self::ObjectReferences {
		&self.object_references
	}

	fn set_stack_references(&mut self, count: usize) {
		self.stack_references = count as u32;
	}

	fn stack_references(&self) -> usize {
		self.stack_references as usize
	}

	fn is_null(&self) -> bool {
		true
	}

	fn cleanup(&mut self) {
		todo!()
	}

	fn convert_to(&self, ty: ItemType) -> Result<StackItemTrait, Err()> {
		if ty == ItemType::Any {
			Ok(StackItem::Null(Self))
		} else {
			Err(())
		}
	}

	fn get_slice(&self) -> &[u8] {
		todo!()
	}

	fn get_string(&self) -> Option<String> {
		None
	}

	fn get_hash_code(&self) -> u64 {
		0
	}

	fn get_type(&self) -> ItemType {
		ItemType::Any
	}

	fn get_boolean(&self) -> bool {
		false
	}
	fn deep_copy(&self, asImmutable: bool) -> Box< StackItem> {
		todo!()
	}
	fn deep_copy_with_ref_map(&self, ref_map: &HashMap<& StackItem, & StackItem>, asImmutable: bool) -> Box< StackItem> {
		todo!()
	}

	fn equals(&self, other: &Option< StackItem>) -> bool {
		todo!()
	}

	fn equals_with_limits(&self, other: & StackItem, limits: &ExecutionEngineLimits) -> bool {
		todo!()
	}

	fn get_integer(&self) -> BigInt {
		todo!()
	}

	fn get_interface<T: 'static>(&self) -> Result<&T, ()> {
		Err(())
	}

	fn get_bytes(&self) -> &[u8] {
		todo!()
	}
}

impl Into< StackItem> for Null {
	fn into(self) -> Box< StackItem> {
		StackItem::Null(self)
	}
}

impl From< StackItem> for Null {
	fn from(item: Box< StackItem>) -> Self {
		match item {
			StackItem::Null(n) => n,
			_ => panic!("Cannot convert {:?} to Null", item),
		}
	}
}
