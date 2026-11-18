#[cfg(target_arch = "riscv32")]
use super::super::helpers::{
    RETAINED_CALL_STACK_BUF, decode_retained_prefix_into, encode_retained_prefix_to_slice,
};
use super::{MAX_CALL_DEPTH, StackValue, call_frame::CallFrame};
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

pub(in crate::interpreter) type RestoredCallFrame = (usize, Vec<StackValue>, Vec<StackValue>, bool);

pub(in crate::interpreter) struct CallStack {
    frames: [core::mem::MaybeUninit<CallFrame>; MAX_CALL_DEPTH],
    len: usize,
    #[cfg(target_arch = "riscv32")]
    retained_len: usize,
}

impl CallStack {
    #[inline]
    pub(in crate::interpreter) fn new() -> Self {
        Self {
            // MaybeUninit slots are valid while uninitialized. Const repeat
            // avoids assume_init on the whole array.
            frames: [const { core::mem::MaybeUninit::uninit() }; MAX_CALL_DEPTH],
            len: 0,
            #[cfg(target_arch = "riscv32")]
            retained_len: 0,
        }
    }

    #[inline]
    pub(in crate::interpreter) fn len(&self) -> usize {
        self.len
    }

    #[cfg(target_arch = "riscv32")]
    pub(in crate::interpreter) fn push_frame_refs(
        &mut self,
        return_ip: usize,
        locals: &[StackValue],
        args: &[StackValue],
        initialized: bool,
    ) -> Result<(), String> {
        if self.len >= MAX_CALL_DEPTH {
            return Err("call depth exceeds maximum".to_string());
        }

        let retained_offset = self.retained_len;
        let buf = unsafe { RETAINED_CALL_STACK_BUF.as_mut_slice() };
        let args_len = encode_retained_prefix_to_slice(args, &mut buf[retained_offset..])?;
        let locals_offset = retained_offset + args_len;
        let locals_len = encode_retained_prefix_to_slice(locals, &mut buf[locals_offset..])?;
        self.retained_len = locals_offset + locals_len;

        self.frames[self.len] = core::mem::MaybeUninit::new(CallFrame {
            return_ip,
            initialized,
            retained_offset,
            args_len,
            locals_len,
        });
        self.len += 1;
        Ok(())
    }

    #[cfg(not(target_arch = "riscv32"))]
    pub(in crate::interpreter) fn push_frame(
        &mut self,
        return_ip: usize,
        locals: Vec<StackValue>,
        args: Vec<StackValue>,
        initialized: bool,
    ) -> Result<(), String> {
        if self.len >= MAX_CALL_DEPTH {
            return Err("call depth exceeds maximum".to_string());
        }

        #[cfg(target_arch = "riscv32")]
        {
            let retained_offset = self.retained_len;
            let buf = unsafe { RETAINED_CALL_STACK_BUF.as_mut_slice() };
            let args_len = encode_retained_prefix_to_slice(&args, &mut buf[retained_offset..])?;
            let locals_offset = retained_offset + args_len;
            let locals_len = encode_retained_prefix_to_slice(&locals, &mut buf[locals_offset..])?;
            self.retained_len = locals_offset + locals_len;

            self.frames[self.len] = core::mem::MaybeUninit::new(CallFrame {
                return_ip,
                initialized,
                #[cfg(target_arch = "riscv32")]
                retained_offset,
                #[cfg(target_arch = "riscv32")]
                args_len,
                #[cfg(target_arch = "riscv32")]
                locals_len,
            });
        }
        #[cfg(not(target_arch = "riscv32"))]
        {
            self.frames[self.len] = core::mem::MaybeUninit::new(CallFrame {
                return_ip,
                initialized,
                #[cfg(not(target_arch = "riscv32"))]
                locals,
                #[cfg(not(target_arch = "riscv32"))]
                args,
            });
        }
        self.len += 1;
        Ok(())
    }

    /// The slot values held by the suspended frames, one slice per slot run.
    ///
    /// The item count has to walk what suspended frames reference as well as
    /// what the active stack, locals, args and static fields reference: a
    /// value can be reachable from both sides at once, and only one shared
    /// deduplicating walk gets that right.
    #[cfg(not(target_arch = "riscv32"))]
    pub(in crate::interpreter) fn saved_values(&self) -> Vec<&[StackValue]> {
        let mut slices = Vec::with_capacity(self.len.saturating_mul(2));
        for frame in &self.frames[..self.len] {
            // SAFETY: every slot below `len` was initialized by a successful
            // push and has not been removed by `pop_and_restore`.
            let frame = unsafe { frame.assume_init_ref() };
            slices.push(frame.locals.as_slice());
            slices.push(frame.args.as_slice());
        }
        slices
    }

    /// The riscv32 guest keeps the suspended frames encoded, so counting has
    /// to decode them. The decoded values are only borrowed for the walk and
    /// are released by the caller.
    #[cfg(target_arch = "riscv32")]
    pub(in crate::interpreter) fn saved_values(&self) -> Result<Vec<Vec<StackValue>>, String> {
        let mut slices = Vec::with_capacity(self.len.saturating_mul(2));
        for frame in &self.frames[..self.len] {
            // SAFETY: every slot below `len` was initialized by a successful
            // push and has not been removed by `pop_and_restore`.
            let frame = unsafe { frame.assume_init_ref() };
            let locals_offset = frame.retained_offset + frame.args_len;
            let end = locals_offset + frame.locals_len;
            let bytes = unsafe { RETAINED_CALL_STACK_BUF.as_slice(end) };

            let mut args = Vec::new();
            decode_retained_prefix_into(
                &bytes[frame.retained_offset..frame.retained_offset + frame.args_len],
                &mut args,
            )?;
            slices.push(args);

            let mut locals = Vec::new();
            decode_retained_prefix_into(&bytes[locals_offset..end], &mut locals)?;
            slices.push(locals);
        }
        Ok(slices)
    }

    pub(in crate::interpreter) fn pop_and_restore(
        &mut self,
    ) -> Result<Option<RestoredCallFrame>, String> {
        if self.len == 0 {
            return Ok(None);
        }

        self.len -= 1;
        // Safety: frames[self.len] was previously initialized by push_frame()
        let frame = unsafe { self.frames[self.len].assume_init_read() };
        #[cfg(target_arch = "riscv32")]
        {
            let locals_offset = frame.retained_offset + frame.args_len;
            let end = locals_offset + frame.locals_len;
            let bytes = unsafe { RETAINED_CALL_STACK_BUF.as_slice(end) };

            let mut args = Vec::new();
            decode_retained_prefix_into(
                &bytes[frame.retained_offset..frame.retained_offset + frame.args_len],
                &mut args,
            )?;

            let mut locals = Vec::new();
            decode_retained_prefix_into(
                &bytes[locals_offset..locals_offset + frame.locals_len],
                &mut locals,
            )?;

            self.retained_len = frame.retained_offset;
            Ok(Some((frame.return_ip, locals, args, frame.initialized)))
        }
        #[cfg(not(target_arch = "riscv32"))]
        {
            Ok(Some((
                frame.return_ip,
                frame.locals,
                frame.args,
                frame.initialized,
            )))
        }
    }
}

impl Drop for CallStack {
    fn drop(&mut self) {
        for frame in &mut self.frames[..self.len] {
            // SAFETY: every slot below `len` was initialized by a successful
            // push and has not been removed by `pop_and_restore`.
            unsafe { frame.assume_init_drop() };
        }
    }
}
