pub mod item_trait;

pub mod compound_types;
pub mod primitive_types;
pub mod stack_item;
pub mod stackitem_type;

use stackitem_type::*;
mod type_error;
mod pointer_item;
mod interop_item;

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
