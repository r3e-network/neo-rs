use crate::{
	item_trait::{ObjectReferenceEntry, StackItem::ByteString, StackItemTrait},
	item_type::ItemType,
	types::compound_types::compound_trait::CompoundTrait,
};
use num_bigint::{BigInt, Sign};
use std::{borrow::Cow, cell::RefCell, collections::HashMap, os::unix::raw::ino_t, vec::Vec};
use crate::compound_types::CompoundItem;
use crate::execution_engine_limits::ExecutionEngineLimits;
use crate::primitive_types::boolean::Boolean;
use crate::primitive_types::byte_string::ByteString;
use crate::primitive_types::primitive_trait::PrimitiveTrait;
use crate::stack_item::StackItem;

#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Buffer {
	stack_references: u32,
	object_references: RefCell<Option<HashMap<CompoundItem, ObjectReferenceEntry>>>,
	dfn: isize,
	low_link: usize,
	on_stack: bool,
	bytes: Cow<'static, [u8]>,
}

impl Buffer {
	pub fn new(size: usize) -> Self {
		Self {
			stack_references: 0,
			object_references: RefCell::new(None),
			dfn: 0,
			low_link: 0,
			on_stack: false,
			bytes: Cow::Owned(Vec::with_capacity(size)),
		}
	}

	// pub fn new_with_init(size:usize, zero_initialize:bool/* = true*/) -> Self
	// {
	// let _buffer = ArrayPool<byte>.Shared.Rent(size);
	// let InnerBuffer = new Memory<byte>(_buffer, 0, size);
	// if (zero_initialize)
	// {
	// 	InnerBuffer.Span.Clear();
	// }
	// }

	pub fn from_slice(data: &[u8]) -> Self {
		Self {
			stack_references: 0,
			object_references: RefCell::new(None),
			dfn: 0,
			low_link: 0,
			on_stack: false,
			bytes: Cow::Borrowed(data),
		}
	}

	fn to_vec(&self) -> Vec<u8> {
		self.bytes.to_vec()
	}

	fn as_slice(&self) -> &[u8] {
		self.bytes.as_ref()
	}
}

impl Drop for Buffer {
	fn drop(&mut self) {
		// Return buffer to pool if not static
	}
}

impl StackItemTrait for Buffer {
	const TRUE: Self = ();

	const FALSE: Self = ();

	const NULL: Self = ();

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
		self.as_slice()
	}

	fn get_type(&self) -> ItemType {
		ItemType::Buffer
	}

	fn get_boolean(&self) -> bool {
		true
	}
	fn deep_copy(
		&self,
		_ref_map: &HashMap<&StackItem, Box< StackItem>>,
		as_immutable: bool,
	) -> Box< StackItem> {
		if as_immutable {
			ByteString::from(self.to_vec()).into()
		} else {
			Buffer::from_slice(self.as_slice()).into()
		}
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

impl PrimitiveTrait for Buffer {
	type Memory = ();

	fn memory(&self) -> &[u8] {
		self.as_slice()
	}

	fn convert_to(&self, ty: ItemType) -> Box< StackItem> {
		match ty {
			ItemType::Integer => {
				if self.bytes.len() > i32::MAX as usize {
					panic!("Invalid cast");
				}
				BigInt::from_bytes_le(Sign::NoSign, self.as_slice()).into()
			},
			ItemType::ByteString => self.to_vec().into(),
			ItemType::Buffer => Buffer::from(self.memory()).into(),
			ItemType::Boolean => Boolean::from(self.get_boolean()).into(),
			_ => panic!("Invalid cast"),
		}
	}
}

impl From<Vec<u8>> for Buffer {
	fn from(bytes: Vec<u8>) -> Self {
		Self {
			stack_references: 0,
			object_references: RefCell::new(None),
			dfn: 0,
			low_link: 0,
			on_stack: false,
			bytes: Cow::Owned(bytes),
		}
	}
}

impl From<&[u8]> for Buffer {
	fn from(bytes: &[u8]) -> Self {
		Self {
			stack_references: 0,
			object_references: RefCell::new(None),
			dfn: 0,
			low_link: 0,
			on_stack: false,
			bytes: Cow::Borrowed(bytes),
		}
	}
}

impl Into< StackItem> for Buffer {
	fn into(self) -> Box< StackItem> {
		StackItem::Buffer(self)
	}
}

impl Into<dyn PrimitiveTrait> for Buffer {
	fn into(self) -> Box<dyn PrimitiveTrait> {
		PrimitiveTrait::VMBuffer(self)
	}
}

impl From<dyn PrimitiveTrait> for Buffer {
	fn from(ty: &dyn PrimitiveTrait) -> Self {
		match ty {
			PrimitiveTrait::VMBuffer(b) => b,
			_ => panic!("Invalid cast"),
		}
	}
}
