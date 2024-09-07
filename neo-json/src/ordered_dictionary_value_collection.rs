
use std::collections::VecDeque;
use std::iter::Iterator;

pub struct ValueCollection<TValue> {
    internal_collection: VecDeque<(TValue, TValue)>,
}

impl<TValue> ValueCollection<TValue> {
    pub fn new(internal_collection: VecDeque<(TValue, TValue)>) -> Self {
        Self { internal_collection }
    }

    pub fn get(&self, index: usize) -> Option<&TValue> {
        self.internal_collection.get(index).map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.internal_collection.len()
    }

    pub fn is_empty(&self) -> bool {
        self.internal_collection.is_empty()
    }

    pub fn contains(&self, item: &TValue) -> bool
    where
        TValue: PartialEq,
    {
        self.internal_collection.iter().any(|(_, v)| v == item)
    }

    pub fn copy_to(&self, array: &mut [TValue], array_index: usize)
    where
        TValue: Clone,
    {
        for (i, (_, value)) in self.internal_collection.iter().enumerate() {
            if i + array_index < array.len() {
                array[i + array_index] = value.clone();
            } else {
                break;
            }
        }
    }
}

impl<TValue> IntoIterator for ValueCollection<TValue> {
    type Item = TValue;
    type IntoIter = std::iter::Map<std::collections::vec_deque::IntoIter<(TValue, TValue)>, fn((TValue, TValue)) -> TValue>;

    fn into_iter(self) -> Self::IntoIter {
        self.internal_collection.into_iter().map(|(_, v)| v)
    }
}

impl<'a, TValue> IntoIterator for &'a ValueCollection<TValue> {
    type Item = &'a TValue;
    type IntoIter = std::iter::Map<std::collections::vec_deque::Iter<'a, (TValue, TValue)>, fn(&'a (TValue, TValue)) -> &'a TValue>;

    fn into_iter(self) -> Self::IntoIter {
        self.internal_collection.iter().map(|(_, v)| v)
    }
}
