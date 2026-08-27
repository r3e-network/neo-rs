use super::{TryFrame, MAX_TRY_NESTING};
use alloc::string::{String, ToString};

pub(in crate::interpreter) struct TryStack {
    frames: [core::mem::MaybeUninit<TryFrame>; MAX_TRY_NESTING],
    pub(in crate::interpreter) len: usize,
}

impl TryStack {
    #[inline]
    pub(in crate::interpreter) fn new() -> Self {
        Self {
            // Safety: MaybeUninit does not require initialization
            frames: unsafe { core::mem::MaybeUninit::uninit().assume_init() },
            len: 0,
        }
    }

    #[inline]
    pub(in crate::interpreter) fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub(in crate::interpreter) fn push(&mut self, frame: TryFrame) -> Result<(), String> {
        if self.len >= MAX_TRY_NESTING {
            return Err("TRY nesting exceeds maximum depth".to_string());
        }
        self.frames[self.len] = core::mem::MaybeUninit::new(frame);
        self.len += 1;
        Ok(())
    }

    #[inline]
    pub(in crate::interpreter) fn pop(&mut self) -> Option<TryFrame> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        // Safety: frames[self.len] was previously initialized by push()
        Some(unsafe { self.frames[self.len].assume_init_read() })
    }

    #[inline]
    pub(in crate::interpreter) fn last_mut(&mut self) -> Option<&mut TryFrame> {
        if self.len == 0 {
            return None;
        }
        // Safety: frames[self.len - 1] was previously initialized by push()
        Some(unsafe { self.frames[self.len - 1].assume_init_mut() })
    }

    /// Find the last uncaught frame index (iterating in reverse)
    pub(in crate::interpreter) fn find_uncaught_index(&self) -> Option<usize> {
        for i in (0..self.len).rev() {
            // Safety: frames[i] was previously initialized by push()
            let frame = unsafe { &*self.frames[i].as_ptr() };
            if !frame.caught {
                return Some(i);
            }
        }
        None
    }

    #[inline]
    pub(in crate::interpreter) fn get_mut(&mut self, index: usize) -> Option<&mut TryFrame> {
        if index >= self.len {
            return None;
        }
        // Safety: frames[index] was previously initialized by push()
        Some(unsafe { self.frames[index].assume_init_mut() })
    }
}
