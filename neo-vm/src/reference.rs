// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use core::hash::{Hash, Hasher};

use crate::*;


pub(crate) enum TrackItem {
    Array(Array),
    Map(Map),
}


impl TrackItem {
    #[inline]
    pub fn with_array(item: Array) -> Self { TrackItem::Array(item) }

    #[inline]
    pub fn with_map(item: Map) -> Self { TrackItem::Map(item) }
}

impl PartialEq<Self> for TrackItem {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        use TrackItem::*;
        match (self, other) {
            (Array(l), Array(r)) => core::ptr::eq(l.as_ptr(), r.as_ptr()),
            (Map(l), Map(r)) => core::ptr::eq(l.as_ptr(), r.as_ptr()),
            _ => false,
        }
    }
}

impl Eq for TrackItem {}

impl Hash for TrackItem {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            TrackItem::Array(v) => v.as_ptr().hash(state),
            TrackItem::Map(v) => v.as_ptr().hash(state),
        }
    }
}

// #[derive(Debug, errors::Error)]
// pub enum ReferenceError {
//     #[error("reference: too depth nested {}, max {}")]
//     TooDepthNested(u32, u32),
// }


// NOTE: References must drop after Vm existed.
pub struct References {
    tracked: hashbrown::HashSet<TrackItem>,
    // zero_referred: hashbrown::HashSet<TrackItem>,
    references: isize,
}

impl References {
    #[inline]
    pub fn new() -> Self {
        Self {
            tracked: Default::default(),
            // zero_referred: Default::default(),
            references: 0,
        }
    }

    // StackItem must add to References before Push to Stack or Slots
    #[inline]
    pub fn add(&mut self, item: &StackItem) { self.recursive_add(item, 1); }

    // StackItem must remove from References after Pop from Stack or Slots
    #[inline]
    pub fn remove(&mut self, item: &StackItem) { self.recursive_remove(item, 1); }

    fn recursive_add(&mut self, item: &StackItem, depth: u32) {
        self.references += 1;
        match item {
            StackItem::Array(v) => {
                if v.strong_count() == 1 {
                    self.tracked.insert(TrackItem::Array(v.clone()));
                    v.items().iter().for_each(|x| self.recursive_add(x, depth + 1));
                }
            }
            StackItem::Struct(v) => {
                v.items().iter().for_each(|x| self.recursive_add(x, depth + 1));
            }
            StackItem::Map(v) => {
                if v.strong_count() == 1 {
                    self.tracked.insert(TrackItem::Map(v.clone()));
                    v.items().iter().for_each(|(_k, v)| self.recursive_add(v, depth + 1));
                }
            }
            _ => {}
        }
    }

    fn recursive_remove(&mut self, item: &StackItem, depth: u32) {
        self.references -= 1;
        match item {
            StackItem::Array(v) => {
                if v.strong_count() == 2 { // 2 == item + another one in self.tracked
                    v.items().iter().for_each(|x| self.recursive_remove(x, depth + 1));
                    self.tracked.remove(&TrackItem::Array(v.clone()));
                }
            }
            StackItem::Struct(v) => {
                v.items().iter().for_each(|x| self.recursive_remove(x, depth + 1));
            }
            StackItem::Map(v) => {
                if v.strong_count() == 2 { // 2 == item + another one in self.tracked
                    v.items().iter().for_each(|(_k, v)| self.recursive_remove(v, depth + 1));
                    self.tracked.remove(&TrackItem::Map(v.clone()));
                }
            }
            _ => {}
        }
    }

    #[inline]
    pub fn references(&self) -> isize { self.references }
}


impl Drop for References {
    fn drop(&mut self) {
        for item in &self.tracked {
            match item {
                TrackItem::Array(v) if v.strong_count() > 1 => v.items_mut().clear(),
                TrackItem::Map(v) if v.strong_count() > 1 => v.items_mut().clear(),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::*;

    #[test]
    fn test_references() {
        let mut rf = References::new();

        let item = StackItem::with_boolean(false);
        rf.add(&item);
        assert_eq!(rf.references(), 1);

        rf.remove(&item);
        assert_eq!(rf.references(), 0);

        let s = StackItem::Array(Array::new(0));
        rf.add(&s);

        let m = StackItem::Map(Map::with_capacity(2));
        rf.add(&m);

        if let StackItem::Array(s) = &s {
            s.items_mut().push(m.clone());
        }

        if let StackItem::Map(m) = &m {
            m.items_mut().insert(StackItem::Integer(1.into()), s.clone());
        }

        assert_eq!(rf.references(), 2);
    }
}
