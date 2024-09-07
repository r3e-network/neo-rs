// Copyright (C) 2015-2024 The Neo Project.
//
// interoperable_list.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo::prelude::*;
use neo::vm::types::{Array, StackItem};
use std::collections::VecDeque;
use std::marker::PhantomData;

/// A trait for types that can be converted to and from StackItems
pub trait InteroperableElement {
    fn from_stack_item(item: StackItem) -> Result<Self, String> where Self: Sized;
    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem;
}

/// An abstract list that implements interoperability with Neo VM stack items
pub struct InteroperableList<T: InteroperableElement> {
    list: VecDeque<T>,
    phantom: PhantomData<T>,
}

impl<T: InteroperableElement> InteroperableList<T> {
    pub fn new() -> Self {
        Self {
            list: VecDeque::new(),
            phantom: PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn push(&mut self, item: T) {
        self.list.push_back(item);
    }

    pub fn pop(&mut self) -> Option<T> {
        self.list.pop_back()
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    pub fn contains(&self, item: &T) -> bool where T: PartialEq {
        self.list.contains(item)
    }

    pub fn index_of(&self, item: &T) -> Option<usize> where T: PartialEq {
        self.list.iter().position(|x| x == item)
    }

    pub fn insert(&mut self, index: usize, item: T) {
        self.list.insert(index, item);
    }

    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index < self.list.len() {
            Some(self.list.remove(index).unwrap())
        } else {
            None
        }
    }

    pub fn sort(&mut self) where T: Ord {
        self.list.make_contiguous().sort();
    }
}

impl<T: InteroperableElement> Interoperable for InteroperableList<T> {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), String> {
        if let StackItem::Array(array) = stack_item {
            self.list.clear();
            for item in array.into_iter() {
                self.list.push_back(T::from_stack_item(item)?);
            }
            Ok(())
        } else {
            Err("Expected Array StackItem".into())
        }
    }

    fn to_stack_item(&self) -> StackItem {
        let mut reference_counter = ReferenceCounter::new();
        StackItem::Array(Array::new(
            self.list
                .iter()
                .map(|item| item.to_stack_item(&mut reference_counter))
                .collect(),
        ))
    }
}

impl<T: InteroperableElement> IntoIterator for InteroperableList<T> {
    type Item = T;
    type IntoIter = std::collections::vec_deque::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.list.into_iter()
    }
}

impl<T: InteroperableElement> FromIterator<T> for InteroperableList<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            list: iter.into_iter().collect(),
            phantom: PhantomData,
        }
    }
}

// Note: Implement additional traits or methods as needed for specific Neo smart contract functionality
