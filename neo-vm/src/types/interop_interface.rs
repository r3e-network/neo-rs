use std::{
	any::{Any, TypeId},
	cell::RefCell,
	collections::HashMap,
	fmt::{Debug, Formatter},
	hash::{Hash, Hasher},
};
use crate::compound_types::compound_trait::CompoundTrait;
use crate::item_trait::{ObjectReferenceEntry, StackItemTrait, StackItemTrait};
use crate::item_type::ItemType;

#[derive(Clone, PartialEq, Eq, Hash, Debug, Default, Copy)]
pub struct InteropInterface {
	stack_references: u32,
	object_references: RefCell<Option<HashMap<CompoundTrait, ObjectReferenceEntry<T>>>>,
	dfn: isize,
	low_link: usize,
	on_stack: bool,
	object: Box<dyn Any>,
}

impl InteropInterface{

}

impl StackItemTrait for InteropInterface {
	type ObjectReferences = ();

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

	fn convert_to(&self, ty: ItemType) -> StackItemTrait {
		todo!()
	}

	fn get_boolean(&self) -> bool {
		true
	}

	fn get_interface<T: Any>(&self) -> Result<&T, InvalidCastError> {
		self.object
			.downcast_ref::<T>()
			.ok_or(InvalidCastError(format!("Cannot cast to {}", std::any::type_name::<T>())))
	}

	fn get_slice(&self) -> &[u8] {
		todo!()
	}

	fn get_type(&self) -> ItemType {
		ItemType::InteropInterface
	}

	fn equals(&self, other: &Option<StackItemTrait>) -> bool {
		match other {
			Some(o) => {
				if self == o.as_ref() {
					return true
				}
				if let Some(i) = o.downcast_ref::<InteropInterface>() {
					self.object.eq(&i.object)
				} else {
					false
				}
			},
			None => false,
		}
	}
}

pub struct InvalidCastError(pub String);

impl Into<StackItemTrait> for InteropInterface {
	fn into(self) -> StackItemTrait {
		StackItemTrait::InteropInterface(self)
	}
}
