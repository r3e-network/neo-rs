//! VM Object Pool for Memory Optimization (Wave 3 - Ivy's implementation)
//! Eliminates per-transaction heap allocations through RAII-managed pooling

use std::cell::Cell;
use std::sync::{Arc, Mutex};
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};

/// Statistics counter for allocation metrics
struct AtomicCounter {
    count: Arc<std::sync::atomic::AtomicUsize>,
}

impl AtomicCounter {
    fn new() -> Self {
        Self {
            count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }
    
    fn increment(&self) {
        self.count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    
    fn value(&self) -> usize {
        self.count.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Object pool reuses instances across transactions
pub struct ObjectPool<T> {
    buffer: Arc<Mutex<Vec<T>>>,
    total_allocated: AtomicCounter,
}

impl<T> ObjectPool<T> {
    /// Create new pool with specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(Vec::with_capacity(capacity))),
            total_allocated: AtomicCounter::new(),
        }
    }
    
    /// Borrow an object from pool (create new if none available)
    pub fn borrow(&self) -> PooledRef<'_, T>
    where
        T: Default,
    {
        let item = match self.buffer.lock().unwrap().pop() {
            Some(existing) => existing,
            None => {
                self.total_allocated.increment();
                T::default()
            }
        };
        
        PooledRef {
            inner: ManuallyDrop::new(item),
            pool: self,
            returned: Cell::new(false),
        }
    }
    
    /// Get total number ever allocated (for metrics)
    pub fn total_allocated(&self) -> usize {
        self.total_allocated.value()
    }
}

/// RAII-managed pooled reference that auto-recycles on drop
pub struct PooledRef<'a, T> {
    inner: ManuallyDrop<T>,
    pool: &'a ObjectPool<T>,
    returned: Cell<bool>,
}

impl<'a, T> PooledRef<'a, T> {
    /// Get mutable reference to inner object
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<'a, T> Deref for PooledRef<'a, T> {
    type Target = T;
    
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a, T> DerefMut for PooledRef<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<'a, T> Drop for PooledRef<'a, T> {
    fn drop(&mut self) {
        if !self.returned.get() {
            // Recycle back to pool instead of deallocating
            // ManuallyDrop::take moves the value out exactly once (drop runs once),
            // so the read is safe here; the item is recycled rather than deallocated.
            let pooled_item = unsafe { ManuallyDrop::take(&mut self.inner) };
            self.pool.buffer.lock().unwrap().push(pooled_item);
        }
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pool_borrow_and_return() {
        let pool: ObjectPool<u64> = ObjectPool::new(5);
        
        {
            let ref1 = pool.borrow();
            assert_eq!(*ref1, 0);
        }
        // Auto-recycles here
        
        {
            let _ref2 = pool.borrow();
            // Should get recycled instance, not allocate new
        }
    }
    
    #[test]
    fn test_pool_reuse() {
        let pool: ObjectPool<String> = ObjectPool::new(3);
        
        let initial_count = pool.total_allocated();
        
        {
            let _ref1 = pool.borrow();
            let _ref2 = pool.borrow();
            let _ref3 = pool.borrow();
        }
        
        let after_count = pool.total_allocated();
        assert_eq!(after_count, initial_count + 3); // Only 3 allocated, rest recycled
        
        // Now reuse
        {
            let _ref4 = pool.borrow();
            let _ref5 = pool.borrow();
        }
        
        let final_count = pool.total_allocated();
        assert_eq!(final_count, after_count); // No new allocations
    }
    
    #[test]
    fn test_explicit_return() {
        let pool: ObjectPool<u64> = ObjectPool::new(2);
        
        let ref1 = pool.borrow();
        ref1.returned.set(true); // Don't recycle
        
        let _ref2 = pool.borrow();
        // This should allocate new since ref1 wasn't recycled
        
        assert_eq!(pool.total_allocated(), 2);
    }
}

impl<'a, T> PooledRef<'a, T> {
    /// Returns whether this pooled reference has been returned to the pool.
    pub fn returned(&self) -> bool {
        self.returned.get()
    }
}
