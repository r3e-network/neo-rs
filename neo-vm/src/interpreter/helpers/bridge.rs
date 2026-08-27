use super::*;

fn stabilize_allocator_after_host_call() {
    if cfg!(target_arch = "riscv32") {
        let _scratch = Vec::<u8>::with_capacity(4096);
    }
}

#[derive(Clone, Copy)]
struct RetainedHostState {
    stack: Option<usize>,
    locals: Option<usize>,
    args: Option<usize>,
    static_fields: Option<usize>,
    consumed_mutations: Option<usize>,
}

impl RetainedHostState {
    fn capture(
        stack: &[StackValue],
        args: &[StackValue],
        locals: &[StackValue],
        static_fields: &[StackValue],
        consumed_mutations: &[StackValue],
    ) -> Result<Self, String> {
        Ok(Self {
            stack: retain_values(stack, &RETAINED_STACK_BUF, true)?,
            locals: retain_values(locals, &RETAINED_LOCALS_BUF, false)?,
            args: retain_values(args, &RETAINED_ARGS_BUF, false)?,
            static_fields: retain_values(static_fields, &RETAINED_STATIC_FIELDS_BUF, false)?,
            consumed_mutations: retain_values(
                consumed_mutations,
                &RETAINED_CONSUMED_MUTATIONS_BUF,
                false,
            )?,
        })
    }

    fn has_stack(self) -> bool {
        self.stack.is_some()
    }

    fn restore_non_stack(
        self,
        consumed_mutations: &mut Vec<StackValue>,
        locals: &mut Vec<StackValue>,
        args: &mut Vec<StackValue>,
        static_fields: &mut Vec<StackValue>,
    ) -> Result<(), String> {
        restore_retained_values(
            consumed_mutations,
            self.consumed_mutations,
            &RETAINED_CONSUMED_MUTATIONS_BUF,
            POST_SYSCALL_STACK_HEADROOM,
        )?;
        restore_retained_values(
            locals,
            self.locals,
            &RETAINED_LOCALS_BUF,
            POST_SYSCALL_STACK_HEADROOM,
        )?;
        restore_retained_values(
            args,
            self.args,
            &RETAINED_ARGS_BUF,
            POST_SYSCALL_STACK_HEADROOM,
        )?;
        restore_retained_values(
            static_fields,
            self.static_fields,
            &RETAINED_STATIC_FIELDS_BUF,
            POST_SYSCALL_STACK_HEADROOM,
        )?;
        Ok(())
    }

    fn restore_non_stack_best_effort(
        self,
        consumed_mutations: &mut Vec<StackValue>,
        locals: &mut Vec<StackValue>,
        args: &mut Vec<StackValue>,
        static_fields: &mut Vec<StackValue>,
    ) {
        let _ = restore_retained_values(
            consumed_mutations,
            self.consumed_mutations,
            &RETAINED_CONSUMED_MUTATIONS_BUF,
            POST_SYSCALL_STACK_HEADROOM,
        );
        let _ = restore_retained_values(
            locals,
            self.locals,
            &RETAINED_LOCALS_BUF,
            POST_SYSCALL_STACK_HEADROOM,
        );
        let _ = restore_retained_values(
            args,
            self.args,
            &RETAINED_ARGS_BUF,
            POST_SYSCALL_STACK_HEADROOM,
        );
        let _ = restore_retained_values(
            static_fields,
            self.static_fields,
            &RETAINED_STATIC_FIELDS_BUF,
            POST_SYSCALL_STACK_HEADROOM,
        );
    }

    fn restore_stack(self, stack: &mut Vec<StackValue>, min_capacity: usize) -> Result<(), String> {
        restore_retained_values(stack, self.stack, &RETAINED_STACK_BUF, min_capacity)
    }

    fn restore_stack_best_effort(self, stack: &mut Vec<StackValue>, min_capacity: usize) {
        let _ = self.restore_stack(stack, min_capacity);
    }
}

fn retain_values(
    values: &[StackValue],
    buf: &RetainedPrefixBuffer,
    retain_empty: bool,
) -> Result<Option<usize>, String> {
    if cfg!(target_arch = "riscv32") && (retain_empty || !values.is_empty()) {
        let buf = unsafe { buf.as_mut_slice() };
        encode_retained_prefix_to_slice(values, buf).map(Some)
    } else {
        let _ = (values, buf);
        Ok(None)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn invoke_syscall<H: SyscallProvider>(
    host: &mut H,
    api: u32,
    ip: usize,
    stack: &mut Vec<StackValue>,
    args: &mut Vec<StackValue>,
    locals: &mut Vec<StackValue>,
    static_fields: &mut Vec<StackValue>,
    consumed_mutations: &mut Vec<StackValue>,
    ids: &mut CompoundIds,
) -> Result<(), String> {
    let arg_count = crate::syscall_arg_count(api).min(stack.len());
    let keep = stack.len() - arg_count;

    let mut abi_args: Vec<crate::StackValue> = Vec::with_capacity(arg_count.min(64));
    for item in &stack[keep..] {
        abi_args.push(to_abi_value(item));
    }

    let retained =
        RetainedHostState::capture(stack, args, locals, static_fields, consumed_mutations)?;
    match host.syscall(api, ip, &mut abi_args) {
        Ok(()) => {
            stabilize_allocator_after_host_call();
            if retained.has_stack() {
                retained.restore_non_stack(consumed_mutations, locals, args, static_fields)?;
                retained.restore_stack(
                    stack,
                    keep.saturating_add(abi_args.len())
                        .max(POST_SYSCALL_STACK_HEADROOM),
                )?;
                stack.truncate(keep);
                stack.reserve(abi_args.len());
                for item in abi_args {
                    stack.push(ids.import_abi(item));
                }
                return Ok(());
            }

            stack.truncate(keep);
            stack.reserve(abi_args.len());
            for item in abi_args {
                stack.push(ids.import_abi(item));
            }
            Ok(())
        }
        Err(e) => {
            if retained.has_stack() {
                retained.restore_non_stack_best_effort(
                    consumed_mutations,
                    locals,
                    args,
                    static_fields,
                );
                retained
                    .restore_stack_best_effort(stack, stack.len().max(POST_SYSCALL_STACK_HEADROOM));
            }
            core::mem::forget(abi_args);
            Err(e)
        }
    }
}

pub(crate) fn complete_initializer_retaining_state<H: SyscallProvider>(
    host: &mut H,
    ip: usize,
    method_initial_stack: &mut Vec<StackValue>,
    static_fields: &mut Vec<StackValue>,
) -> Result<(), String> {
    let retained_method_stack_len =
        retain_values(method_initial_stack, &RETAINED_STACK_BUF, false)?;
    let retained_static_fields_len =
        retain_values(static_fields, &RETAINED_STATIC_FIELDS_BUF, false)?;

    let result = host.initializer_complete(ip);
    if result.is_ok() {
        stabilize_allocator_after_host_call();
    }

    if retained_method_stack_len.is_some() || retained_static_fields_len.is_some() {
        let method_restore = restore_retained_values(
            method_initial_stack,
            retained_method_stack_len,
            &RETAINED_STACK_BUF,
            method_initial_stack.len().max(POST_SYSCALL_STACK_HEADROOM),
        );
        let static_restore = restore_retained_values(
            static_fields,
            retained_static_fields_len,
            &RETAINED_STATIC_FIELDS_BUF,
            static_fields.len().max(POST_SYSCALL_STACK_HEADROOM),
        );

        if let Err(error) = result {
            let _ = method_restore;
            let _ = static_restore;
            return Err(error);
        }

        method_restore?;
        static_restore?;
        return Ok(());
    }

    result
}

pub(crate) fn retain_initializer_method_stack(
    method_initial_stack: &[StackValue],
) -> Result<Option<usize>, String> {
    #[cfg(target_arch = "riscv32")]
    {
        if method_initial_stack.is_empty() {
            return Ok(None);
        }
        let buf = unsafe { RETAINED_INITIAL_STACK_BUF.as_mut_slice() };
        return encode_retained_prefix_to_slice(method_initial_stack, buf).map(Some);
    }
    #[cfg(not(target_arch = "riscv32"))]
    {
        let _ = method_initial_stack;
        Ok(None)
    }
}

pub(crate) fn restore_initializer_method_stack(
    method_initial_stack: &mut Vec<StackValue>,
    retained_len: Option<usize>,
) -> Result<(), String> {
    #[cfg(target_arch = "riscv32")]
    {
        restore_retained_values(
            method_initial_stack,
            retained_len,
            &RETAINED_INITIAL_STACK_BUF,
            method_initial_stack.len().max(POST_SYSCALL_STACK_HEADROOM),
        )
    }
    #[cfg(not(target_arch = "riscv32"))]
    {
        let _ = (method_initial_stack, retained_len);
        Ok(())
    }
}

fn restore_retained_values(
    values: &mut Vec<StackValue>,
    retained_len: Option<usize>,
    buf: &RetainedPrefixBuffer,
    min_capacity: usize,
) -> Result<(), String> {
    match retained_len {
        Some(retained_len) => {
            let retired = core::mem::take(values);
            core::mem::forget(retired);

            let mut restored = Vec::with_capacity(min_capacity);
            decode_retained_prefix_into(unsafe { buf.as_slice(retained_len) }, &mut restored)?;
            *values = restored;
            Ok(())
        }
        None => Ok(()),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn invoke_callt<H: SyscallProvider>(
    host: &mut H,
    token: u16,
    ip: usize,
    stack: &mut Vec<StackValue>,
    args: &mut Vec<StackValue>,
    locals: &mut Vec<StackValue>,
    static_fields: &mut Vec<StackValue>,
    consumed_mutations: &mut Vec<StackValue>,
    ids: &mut CompoundIds,
) -> Result<(), String> {
    let mut abi_stack = to_abi_stack(stack);
    let retained =
        RetainedHostState::capture(stack, args, locals, static_fields, consumed_mutations)?;
    match host.callt(token, ip, &mut abi_stack) {
        Ok(()) => {
            stabilize_allocator_after_host_call();
            if retained.has_stack() {
                retained.restore_non_stack(consumed_mutations, locals, args, static_fields)?;
                retained.restore_stack(stack, stack.len().max(POST_SYSCALL_STACK_HEADROOM))?;
            }

            let mut imported_stack =
                Vec::with_capacity(abi_stack.len().max(POST_SYSCALL_STACK_HEADROOM));
            for item in abi_stack {
                let imported = ids.import_abi(item);
                let stabilized = if cfg!(target_arch = "riscv32")
                    && matches!(
                        imported,
                        StackValue::Array(..)
                            | StackValue::Struct(..)
                            | StackValue::Map(..)
                            | StackValue::Buffer(..)
                    ) {
                    ids.deep_clone(&imported)
                } else {
                    imported
                };
                imported_stack.push(stabilized);
            }
            let shared_prefix = stack
                .iter()
                .zip(imported_stack.iter())
                .take_while(|(left, right)| structurally_equal(left, right))
                .count();
            #[cfg(target_arch = "riscv32")]
            {
                let mut preserved_stack = core::mem::take(stack);
                let mut next_stack =
                    Vec::with_capacity(imported_stack.len().max(POST_SYSCALL_STACK_HEADROOM));

                if shared_prefix > 0 {
                    let discarded_preserved = preserved_stack.split_off(shared_prefix);
                    next_stack.append(&mut preserved_stack);
                    core::mem::forget(discarded_preserved);
                } else {
                    core::mem::forget(preserved_stack);
                }

                let mut imported_suffix = imported_stack.split_off(shared_prefix);
                next_stack.append(&mut imported_suffix);
                core::mem::forget(imported_stack);
                *stack = next_stack;
            }
            #[cfg(not(target_arch = "riscv32"))]
            {
                imported_stack[..shared_prefix].clone_from_slice(&stack[..shared_prefix]);
                *stack = imported_stack;
            }
            Ok(())
        }
        Err(e) => {
            if retained.has_stack() {
                retained.restore_non_stack_best_effort(
                    consumed_mutations,
                    locals,
                    args,
                    static_fields,
                );
                retained
                    .restore_stack_best_effort(stack, stack.len().max(POST_SYSCALL_STACK_HEADROOM));
            }
            // Leak abi_stack on error to avoid talc free-list corruption on RISC-V/PolkaVM
            core::mem::forget(abi_stack);
            Err(e)
        }
    }
}
