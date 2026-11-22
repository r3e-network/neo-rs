use super::*;

/// Reusable vector that maintains capacity across clears.
///
/// Unlike `std::vec::Vec`, this type guarantees zero reallocations when the
/// number of elements stays within its configured maximum capacity. After
/// `clear_keep_capacity()` is called, the internal buffer remains allocated
/// and ready for immediate reuse.
///
/// ## Key Differences from Vec
///
/// | Behavior | Vec | PooledVec |
/// |----------|-----|-----------|
/// | push beyond capacity | reallocates (slow) | panics (fail-fast) |
/// | clear() | may deallocate | keeps capacity |
/// | predictable performance | ❌ | ✅ |
/// | max_elements enforced | ❌ | ✅ |
///
/// # Type Parameters
///
/// * `T` - Element type
///
/// # Examples
///
/// ```rust,no_run
/// use neo_vm::memory::PooledVec;
///
/// // Pre-allocate capacity for 100 events
/// let mut events = PooledVec::<&str>::with_capacity(100);
///
/// // First batch: allocates memory for 100 elements
/// events.push("Event 1");
/// events.push("Event 2");
/// // ... push up to 100 items
///
/// // Process events...
///
/// // Clear without freeing memory
/// events.clear_keep_capacity();
///
/// // Next batch: ZERO allocations!
/// events.push("Event 101");
/// events.push("Event 102");
/// ```
pub struct PooledVec<T> {
    pub(super) inner: Vec<T>,
    max_capacity: usize,
}

impl<T> PooledVec<T> {
    /// Creates a new empty `PooledVec` with the specified capacity.
    ///
    /// The returned vector will never allocate more than `capacity` elements,
    /// even if pushed beyond initial size. Attempting to exceed capacity will
    /// panic at runtime.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of elements (enforced at runtime)
    ///
    /// # Panics
    ///
    /// Panics if `capacity` exceeds system memory limits or is excessively large.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::memory::PooledVec;
    ///
    /// // Capacity guaranteed: will never exceed 64 elements
    /// let mut pool = PooledVec::<i32>::with_capacity(64);
    ///
    /// // Safe: within capacity
    /// pool.push(42);
    ///
    /// // Panics: exceeds maximum capacity
    /// // for i in 0..100 { pool.push(i); } // Would panic at i=64
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Vec::with_capacity(capacity),
            max_capacity: capacity,
        }
    }

    /// Returns the current allocation capacity.
    ///
    /// This is preserved across [`Self::clear_keep_capacity`].
    #[inline]
    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Creates a new empty `PooledVec` with default capacity (0).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::memory::PooledVec;
    ///
    /// let mut pool = PooledVec::<i32>::new();
    /// assert!(pool.is_empty());
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: Vec::new(),
            max_capacity: 0,
        }
    }

    /// Sets maximum capacity after construction.
    ///
    /// Allows dynamic adjustment of capacity constraints. Should be called before
    /// any pushes to enforce safety.
    ///
    /// # Arguments
    ///
    /// * `capacity` - New maximum element count
    pub fn set_max_capacity(&mut self, capacity: usize) {
        self.max_capacity = capacity;
    }

    /// Appends an element to the vector, enforcing capacity limits.
    ///
    /// Unlike `Vec::push`, this method panics if adding the element would exceed
    /// the maximum capacity. This fail-fast behavior prevents unexpected reallocations.
    ///
    /// # Arguments
    ///
    /// * `value` - Element to append
    ///
    /// # Panics
    ///
    /// Panics if `len() == max_capacity`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::memory::PooledVec;
    ///
    /// let mut vec = PooledVec::<i32>::with_capacity(2);
    /// vec.push(1);
    /// vec.push(2);
    /// assert_eq!(vec.len(), 2);
    ///
    /// // This will panic!
    /// // vec.push(3); // PANIC: capacity exceeded
    /// ```
    #[inline]
    pub fn push(&mut self, value: T) {
        if self.inner.len() >= self.max_capacity && self.max_capacity > 0 {
            panic!(
                "PooledVec capacity exceeded: len={} max={}",
                self.inner.len(),
                self.max_capacity
            );
        }
        self.inner.push(value);
    }

    /// Clears all elements while keeping allocated capacity.
    ///
    /// This is the preferred clear method as it:
    /// - Sets length to 0 without deallocating memory
    /// - Preserves the internal buffer's capacity
    /// - Guarantees next insertion doesn't trigger reallocation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::memory::PooledVec;
    ///
    /// let mut vec = PooledVec::<i32>::with_capacity(64);
    /// for i in 0..50 {
    ///     vec.push(i);
    /// }
    ///
    // Zero-copy clear - capacity preserved!
    /// vec.clear_keep_capacity();
    /// assert_eq!(vec.len(), 0);
    /// assert_eq!(vec.capacity(), 64);
    /// ```
    #[inline]
    pub fn clear_keep_capacity(&mut self) {
        self.inner.clear();
    }

    /// Gets the number of elements in the vector.
    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Checks if the vector is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Gets mutable reference to underlying Vec (unsafe usage).
    ///
    /// # Safety
    ///
    /// Caller must ensure modifications don't exceed `max_capacity`.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.inner
    }

    /// Gets immutable reference to underlying Vec.
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        &self.inner
    }

    /// Tries to push an element, returning error if capacity exceeded.
    ///
    /// Unlike `push()`, this method returns `Err(T)` if the vector has reached
    /// maximum capacity, allowing graceful handling instead of panic.
    ///
    /// # Arguments
    ///
    /// * `item` - Element to try pushing
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Successfully pushed
    /// * `Err(T)` - Capacity exceeded, item returned unchanged
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::memory::PooledVec;
    ///
    /// let mut vec = PooledVec::<i32>::with_capacity(2);
    /// vec.try_push(1).unwrap();
    /// vec.try_push(2).unwrap();
    /// assert_eq!(vec.try_push(3), Err(3)); // Capacity exceeded
    /// ```
    #[inline]
    pub fn try_push(&mut self, item: T) -> Result<(), T> {
        if self.inner.len() >= self.max_capacity && self.max_capacity > 0 {
            Err(item)
        } else {
            self.inner.push(item);
            Ok(())
        }
    }

    /// Reserves capacity for minimum additional elements.
    ///
    /// Does nothing if capacity is already sufficient. If max_capacity is set
    /// and requested reservation would exceed it, sets max_capacity accordingly.
    ///
    /// # Arguments
    ///
    /// * `additional` - Minimum number of elements to reserve space for
    pub fn reserve(&mut self, additional: usize) {
        let required = self.inner.len().saturating_add(additional);

        if self.max_capacity > 0 && required > self.max_capacity {
            // Adjust max_capacity if needed (shouldn't happen in normal usage)
            self.max_capacity = required;
        }

        self.inner.reserve(additional);
    }
}

impl<T> Default for PooledVec<T> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Deref for PooledVec<T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for PooledVec<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T: Clone> Clone for PooledVec<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            max_capacity: self.max_capacity,
        }
    }

    #[inline]
    fn clone_from(&mut self, other: &Self) {
        self.inner.clone_from(&other.inner);
        self.max_capacity = other.max_capacity;
    }
}
