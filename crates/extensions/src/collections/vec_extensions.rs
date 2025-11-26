use std::cmp::Ordering;
use std::collections::HashSet;
use std::hash::Hash;

/// Vec extensions that mirror the C# Neo.Extensions collection helpers.
pub trait VecExtensions<T> {
    /// Remove duplicate elements while preserving the original order.
    fn dedup_preserve_order(&mut self)
    where
        T: Eq + Hash + Clone;

    /// Split the vector into chunks of `chunk_size`, keeping the final partial chunk.
    fn chunks_exact_vec(&self, chunk_size: usize) -> Vec<Vec<T>>
    where
        T: Clone;

    /// Return the index of the maximum element, or None if empty.
    fn argmax(&self) -> Option<usize>
    where
        T: PartialOrd;

    /// Return the index of the minimum element, or None if empty.
    fn argmin(&self) -> Option<usize>
    where
        T: PartialOrd;

    /// Check if the vector is sorted in non-decreasing order.
    fn is_sorted(&self) -> bool
    where
        T: PartialOrd;

    /// Borrow the first `n` items (or fewer if the vector is shorter).
    fn first_n(&self, n: usize) -> &[T];

    /// Borrow the last `n` items (or fewer if the vector is shorter).
    fn last_n(&self, n: usize) -> &[T];
}

impl<T> VecExtensions<T> for Vec<T> {
    fn dedup_preserve_order(&mut self)
    where
        T: Eq + Hash + Clone,
    {
        let mut seen = HashSet::new();
        self.retain(|item| seen.insert(item.clone()));
    }

    fn chunks_exact_vec(&self, chunk_size: usize) -> Vec<Vec<T>>
    where
        T: Clone,
    {
        if chunk_size == 0 {
            return Vec::new();
        }

        let mut chunks = Vec::new();
        let len = self.len();
        let mut start = 0;

        while start < len {
            let end = (start + chunk_size).min(len);
            chunks.push(self[start..end].to_vec());
            start = end;
        }

        chunks
    }

    fn argmax(&self) -> Option<usize>
    where
        T: PartialOrd,
    {
        let mut best: Option<(usize, &T)> = None;

        for (idx, value) in self.iter().enumerate() {
            match best {
                None => best = Some((idx, value)),
                Some((_, current)) => {
                    if let Some(Ordering::Greater) = value.partial_cmp(current) {
                        best = Some((idx, value));
                    }
                }
            }
        }

        best.map(|(idx, _)| idx)
    }

    fn argmin(&self) -> Option<usize>
    where
        T: PartialOrd,
    {
        let mut best: Option<(usize, &T)> = None;

        for (idx, value) in self.iter().enumerate() {
            match best {
                None => best = Some((idx, value)),
                Some((_, current)) => {
                    if let Some(Ordering::Less) = value.partial_cmp(current) {
                        best = Some((idx, value));
                    }
                }
            }
        }

        best.map(|(idx, _)| idx)
    }

    fn is_sorted(&self) -> bool
    where
        T: PartialOrd,
    {
        if self.len() <= 1 {
            return true;
        }

        self.windows(2).all(|pair| match pair {
            [a, b] => b.partial_cmp(a).map_or(true, |ord| ord != Ordering::Less),
            _ => true,
        })
    }

    fn first_n(&self, n: usize) -> &[T] {
        let end = n.min(self.len());
        &self[..end]
    }

    fn last_n(&self, n: usize) -> &[T] {
        let len = self.len();
        let start = len.saturating_sub(n);
        &self[start..]
    }
}
