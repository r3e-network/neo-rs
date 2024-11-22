// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use crate::compound_types::compound_trait::CompoundTrait;
use crate::stack_item::{SharedItem, StackItem};

pub struct References {
    references: isize,
}

impl References {
    #[inline]
    pub fn new() -> Self {
        Self {
            references: 0,
        }
    }

    // Add an item to the reference counter
    #[inline]
    pub fn add(&mut self, item: &SharedItem) {
        if let Ok(guard) = item.lock() {
            self.references += 1;

            match &*guard {
                StackItem::Array(arr) => {
                    if arr.ref_inc() == 1 {
                        for item in arr.items() {
                            self.add(item);
                        }
                    }
                }
                StackItem::Struct(s) => {
                    if Arc::strong_count(item) == 1 {
                        for item in s.items() {
                            self.add(item);
                        }
                    }
                }
                StackItem::Map(m) => {
                    if Arc::strong_count(item) == 1 {
                        for (key, value) in m.items() {
                            self.add(key);
                            self.add(value);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Remove an item from the reference counter
    #[inline] 
    pub fn remove(&mut self, item: &SharedItem) {
        if let Ok(guard) = item.lock() {
            self.references -= 1;

            match &*guard {
                StackItem::Array(arr) => {
                    if Arc::strong_count(item) == 0 {
                        for item in arr.items() {
                            self.remove(item);
                        }
                    }
                }
                StackItem::Struct(s) => {
                    if Arc::strong_count(item) == 0 {
                        for item in s.items() {
                            self.remove(item);
                        }
                    }
                }
                StackItem::Map(m) => {
                    if Arc::strong_count(item) == 0 {
                        for (key, value) in m.items() {
                            self.remove(key);
                            self.remove(value);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    #[inline]
    pub fn references(&self) -> isize {
        self.references
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::stack_item::{ArrayItem, MapItem};

    #[test]
    fn test_references() {
        let mut rf = References::new();

        let item = StackItem::with_boolean(false).into_arc_mutex();
        rf.add(&item);
        assert_eq!(rf.references(), 1);

        rf.remove(&item);
        assert_eq!(rf.references(), 0);

        let array = StackItem::Array(ArrayItem::new(0)).into_arc_mutex();
        let map = StackItem::Map(MapItem::with_capacity(2)).into_arc_mutex();

        rf.add(&array);
        rf.add(&map);

        if let Ok(mut arr) = array.lock() {
            if let StackItem::Array(a) = &mut *arr {
                a.items_mut().push(map.clone());
            }
        }

        if let Ok(mut m) = map.lock() {
            if let StackItem::Map(m) = &mut *m {
                m.items_mut().insert(
                    StackItem::Integer(1.into()), 
                    array.clone()
                );
            }
        }

        assert_eq!(rf.references(), 2);
    }
}
