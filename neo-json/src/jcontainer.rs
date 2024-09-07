
use crate::jtoken::JToken;
use std::ops::Index;

pub trait JContainer: JToken {
    fn children(&self) -> &[Option<Box<dyn JToken>>];

    fn count(&self) -> usize {
        self.children().len()
    }

    fn clear(&mut self);

    fn copy_to(&self, array: &mut [Option<Box<dyn JToken>>], array_index: usize) {
        for (i, child) in self.children().iter().enumerate() {
            if i + array_index >= array.len() {
                break;
            }
            array[i + array_index] = child.clone();
        }
    }
}

impl<T: JContainer> Index<usize> for T {
    type Output = Option<Box<dyn JToken>>;

    fn index(&self, index: usize) -> &Self::Output {
        &self.children()[index]
    }
}
