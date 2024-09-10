#![feature(linked_list_remove)]

use std::{
	cell::RefCell,
	cmp::Eq,
	collections::{HashMap, HashSet, LinkedList},
	hash::{Hash, Hasher},
	marker::PhantomData,
	rc::Rc,
};
use crate::compound_types::compound_trait::CompoundTrait;
use crate::stack_item::StackItem;

#[derive(Debug)]
pub struct ReferenceEntry<T>
where
	T: CompoundTrait,
{
	item: T,
	references: usize,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReferenceCounter {
	tracked_items: HashSet<Rc<RefCell<StackItem>>>,
	zero_referred: HashSet<Rc<RefCell<StackItem>>>,
	cached_components: Option<LinkedList<HashSet<Rc<RefCell<StackItem>>>>>,
	references_count: usize,
	phantom: PhantomData<Rc<RefCell<StackItem>>>,
}

impl ReferenceCounter {
	pub fn new() -> Self {
		Self {
			tracked_items: HashSet::new(),
			zero_referred: HashSet::new(),
			cached_components: None,
			references_count: 0,
			phantom: PhantomData,
		}
	}

	fn need_track(&self, item: Rc<RefCell< StackItem>>) -> bool {
		// Track compound types and buffers
		if let StackItemTrait::CompoundType(_) | StackItemTrait::Buffer(_) = item {
			true
		} else {
			false
		}
	}

	pub(crate) fn add_reference(
		&mut self,
		item: Rc<RefCell< StackItem>>,
		parent: &CompoundItem
	) {
		self.references_count += 1;
		if !self.need_track(item) {
			return
		}

		self.cached_components = None;

		self.tracked_items.insert(item.clone());

		if let Some(refs) = &mut item.borrow().get_stack_item().object_references {
			if let Some(entry) = refs.get_mut(parent) {
				entry.references += 1;
			} else {
				refs.insert(parent.clone(), ReferenceEntry { item: parent.clone(), references: 1 });
			}
		}
	}

	pub(crate) fn add_stack_reference(
		&mut self,
		mut item: Rc<RefCell< StackItem>>,
		count: usize, /* = 1*/
	) {
		self.references_count += count;

		if !self.need_track(item.clone()) {
			return
		}

		if self.tracked_items.insert(item.clone()) {
			self.cached_components
				.as_mut()
				.map(|components| components.push_back(HashSet::from([item.clone()])));
		}

		item.get_mut().set_stack_references(item.borrow().stack_references() + count);
		self.zero_referred.remove(&item);
	}

	fn add_zero_referred(&mut self, item: Rc<RefCell< StackItem>>) {
		self.zero_referred.insert(item.clone());

		if !self.need_track(item) {
			return
		}

		self.cached_components
			.as_mut()
			.map(|components| components.push_back(HashSet::from([item.clone()])));

		self.tracked_items.insert(item.clone());
	}


	fn check_zero_referred(&mut self) -> usize {
		if !self.zero_referred.is_empty() {
			self.zero_referred.clear();

			let mut components = self.cached_components.get_or_insert_with(|| LinkedList::new());

			for item in &self.tracked_items {
				item.reset();
			}

			let mut node = components.front_mut();
			while let Some(component) = node {
				let mut on_stack = false;

				for item in &component {
					if item.stack_references > 0
						|| item
							.object_references
							.as_ref()
							.map(|refs| {
								refs.values()
									.any(|entry| entry.references > 0 && entry.item.on_stack)
							})
							.unwrap_or(false)
					{
						on_stack = true;
						break
					}
				}

				if on_stack {
					for item in &component {
						item.on_stack = true;
					}
					node = node.next_mut();
				} else {
					for item in &component {
						self.tracked_items.remove(item);

						if let StackItemTrait::CompoundType(compound) = item {
							self.references_count -= compound.sub_items.len();

							for subitem in &compound.sub_items {
								if component.contains(subitem) {
									continue
								}

								if self.need_track(subitem) {
									subitem.object_references.as_mut().map(|refs| {
										refs.remove(&compound);
									});
								}
							}
						}

						item.cleanup();
					}

					let node_to_remove = node.take().unwrap();
					let pos = components.iter().position(|&x| &x == node_to_remove).unwrap();
					// #![feature(linked_list_remove)]
					components.remove(pos);
				}
			}
		}

		self.references_count
	}

	fn remove_reference(
		&mut self,
		item: Rc<RefCell< StackItem>>,
		parent: &CompoundItem
	) {
		self.references_count -= 1;

		if !self.need_track(item) {
			return
		}

		self.cached_components = None;

		if let Some(refs) = &mut item.object_references() {
			if let Some(entry) = refs.get_mut(parent) {
				entry.references -= 1;
			}
		}

		if item.stack_references() == 0 {
			self.zero_referred.insert(item.clone());
		}
	}

	pub(crate) fn remove_stack_reference(&mut self, mut item: Rc<RefCell< StackItem>>) {
		self.references_count -= 1;

		if !self.need_track(item.clone()) {
			return
		}

		let mut it = item.get_mut().get_stack_item();
		it.set_stack_references(it.stack_references() - 1);
		if it.stack_references() == 0 {
			self.zero_referred.insert(item.clone());
		}
	}

	pub fn count(&self) -> usize {
		self.references_count
	}
}
