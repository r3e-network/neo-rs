pub mod interop_interface;
pub mod reference_counter;
pub mod item_trait;
pub mod item_type;

pub mod buffer;

pub mod null;

pub mod pointer;

pub mod compound_types;
pub mod primitive_types;
pub mod stack_item;

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
