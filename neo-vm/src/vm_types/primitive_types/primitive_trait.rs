use std::cell::RefCell;
use std::collections::HashMap;
use serde::__private::de::Content::String;
use crate::{
	item_trait::{StackItemTrait},
};
use crate::buffer::Buffer;
use crate::primitive_types::boolean::Boolean;
use crate::primitive_types::byte_string::ByteString;
use crate::primitive_types::integer::Integer;
use crate::item_type::StackItemType;

pub trait PrimitiveTrait: StackItemTrait + Clone {

	type Memory;

	fn memory(&self) -> &[u8];

	/// The size of the vm object in bytes.
	fn size(&self) -> usize {
		self.memory().len()
	}

	fn convert_to(&self, type_: StackItemType) -> Result<Self, Err>  {
		match type_ {
			StackItemType::Integer => Ok(Integer::from(self.get_integer())),
			StackItemType::ByteString =>  Ok(ByteString::from( String::from_utf8(self.memory())?)),
			StackItemType::Buffer =>  Ok(Buffer::from(self.get_slice()).into()),
			StackItemType::Boolean =>  Ok(Boolean::from(self.get_boolean().into()).into()),
			_ => panic!(), //self.base_convert_to(ty),
		}
	}

	fn deep_copy_with_ref_map(&self, ref_map: &HashMap<& StackItem, & StackItem>) -> Box< StackItem> {
		self.clone()
	}

	fn get_slice(&self) -> &[u8]{
		self.memory()
	}
}
