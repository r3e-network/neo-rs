use std::{cell::RefCell, collections::HashMap, hash::Hash};
use num_bigint::BigInt;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
	item_trait::{ObjectReferenceEntry, StackItemTrait},
	item_type::ItemType,
	vm::script::Script,
};
use crate::compound_types::compound_trait::CompoundTrait;
use crate::execution_engine_limits::ExecutionEngineLimits;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Pointer {
	stack_references: u32,
	object_references: RefCell<Option<HashMap<CompoundItem ObjectReferenceEntry>>>,
	dfn: isize,
	low_link: usize,
	on_stack: bool,
	script: Script,
	position: usize,
}

impl Pointer {
	pub fn new(script: &Script, position: usize) -> Self {
		Self {
			stack_references: 0,
			object_references: RefCell::new(None),
			dfn: 0,
			low_link: 0,
			on_stack: false,
			script:script.clone(),
			position,
		}
	}

	pub fn script(&self) -> &Script {
		&self.script
	}

	pub fn position(&self) -> usize {
		self.position
	}
}

impl PartialEq<Self> for Pointer {
	fn eq(&self, other: &Self) -> bool {
		todo!()
	}
}

impl Serialize for Pointer {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
	}
}

impl Deserialize for Pointer {
	fn deserialize<'de, D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
		todo!()
	}
}

impl StackItemTrait for Pointer {
	const TRUE: Self = Default::default();

	const FALSE: Self = Default::default();

	const NULL: Self = Default::default();

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

	fn cleanup(&mut self) {
		todo!()
	}

	fn get_slice(&self) -> &[u8] {
		todo!()
	}

	fn get_type(&self) -> ItemType {
		ItemType::Pointer
	}
	fn get_boolean(&self) -> bool {
		true
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

	fn get_bytes(&self) -> &[u8] {
		todo!()
	}
}

// fn equals(&self, other: &Option<StackItem>) -> bool {
//     match other {
//         Some(o) => self == o.as_ref().downcast_ref::<Pointer>().unwrap(),
//         None => false,
//     }
// }
//
// fn get_boolean(&self) -> bool {
//     true
// }
