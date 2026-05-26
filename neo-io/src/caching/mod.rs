//! Caching module - matches C# Neo.IO.Caching exactly

macro_rules! impl_cache_wrapper_deref {
    (
        impl<$($generic:ident),+> for $wrapper:ty
        where { $($bounds:tt)* }
        => $target:ty
    ) => {
        impl<$($generic),+> ::std::ops::Deref for $wrapper
        where
            $($bounds)*
        {
            type Target = $target;

            fn deref(&self) -> &Self::Target {
                &self.inner
            }
        }

        impl<$($generic),+> ::std::ops::DerefMut for $wrapper
        where
            $($bounds)*
        {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.inner
            }
        }
    };
}

macro_rules! impl_cache_facade {
    () => {
        /// Gets the number of cached entries (C# Count property).
        #[inline]
        pub fn count(&self) -> usize {
            self.entries.lock().len()
        }

        /// Indicates whether the cache is empty.
        #[inline]
        pub fn is_empty(&self) -> bool {
            self.count() == 0
        }

        /// Indicates whether the cache is read-only (always false in C# implementation).
        #[inline]
        pub const fn is_read_only(&self) -> bool {
            false
        }

        /// Adds a range of items to the cache.
        pub fn add_range<I>(&self, items: I)
        where
            I: IntoIterator<Item = TValue>,
        {
            for item in items {
                self.add(item);
            }
        }

        /// Clears the cache.
        pub fn clear(&self) {
            self.entries.lock().clear();
        }

        /// Determines whether the cache contains the specified item.
        pub fn contains(&self, item: &TValue) -> bool {
            let key = (self.key_selector)(item);
            self.contains_key(&key)
        }

        /// Copies cache contents to the provided slice.
        ///
        /// # Errors
        ///
        /// Returns an error if `start_index` is outside `destination`, if the
        /// copy range overflows, or if `destination` does not have enough
        /// space for all cached values.
        pub fn copy_to(
            &self,
            destination: &mut [TValue],
            start_index: usize,
        ) -> crate::IoResult<()> {
            self.entries.lock().copy_to(destination, start_index)
        }

        /// Removes an item by key.
        ///
        /// Returns `true` if the item was found and removed, `false` otherwise.
        pub fn remove_key(&self, key: &TKey) -> bool {
            self.entries.lock().remove(key)
        }

        /// Removes an item.
        ///
        /// Returns `true` if the item was found and removed, `false` otherwise.
        pub fn remove(&self, item: &TValue) -> bool {
            let key = (self.key_selector)(item);
            self.remove_key(&key)
        }

        /// Attempts to retrieve an item by key.
        #[inline]
        pub fn try_get(&self, key: &TKey) -> Option<TValue> {
            self.get(key)
        }

        /// Returns a snapshot of cache values from oldest/least-recent to newest/most-recent.
        pub fn values(&self) -> Vec<TValue> {
            self.entries.lock().values()
        }

        /// Maximum number of elements allowed in the cache.
        #[inline]
        pub const fn max_capacity(&self) -> usize {
            self.max_capacity
        }
    };
}

pub mod cache;
pub(crate) mod cache_entries;
pub mod ec_point_cache;
pub mod ecdsa_cache;
pub mod fifo_cache;
pub mod hashset_cache;
pub mod lru_cache;
pub mod relay_cache;
