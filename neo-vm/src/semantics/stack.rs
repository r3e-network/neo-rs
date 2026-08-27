//! Pure stack-shape semantics shared by runtime adapters and the interpreter.

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

pub(crate) fn depth<T>(stack: &[T]) -> i64 {
    stack.len() as i64
}

pub(crate) fn drop_top<T>(stack: &mut Vec<T>) -> Result<(), String> {
    stack
        .pop()
        .map(|_| ())
        .ok_or_else(|| "stack underflow: DROP".to_string())
}

pub(crate) fn dup<T: Clone>(stack: &mut Vec<T>) -> Result<(), String> {
    let value = stack
        .last()
        .cloned()
        .ok_or_else(|| "stack underflow: DUP".to_string())?;
    stack.push(value);
    Ok(())
}

pub(crate) fn swap<T>(stack: &mut [T]) -> Result<(), String> {
    let len = stack.len();
    if len < 2 {
        return Err("stack underflow: SWAP requires at least 2 items".to_string());
    }
    stack.swap(len - 1, len - 2);
    Ok(())
}

pub(crate) fn nip<T>(stack: &mut Vec<T>) -> Result<(), String> {
    let len = stack.len();
    if len < 2 {
        return Err("stack underflow: NIP requires at least 2 items".to_string());
    }
    stack.remove(len - 2);
    Ok(())
}

pub(crate) fn xdrop<T>(stack: &mut Vec<T>, index: i64) -> Result<(), String> {
    let index = non_negative_index("XDROP", index)?;
    let len = stack.len();
    if index >= len {
        return Err("XDROP: index out of range".to_string());
    }
    stack.remove(len - 1 - index);
    Ok(())
}

pub(crate) fn over<T: Clone>(stack: &mut Vec<T>) -> Result<(), String> {
    let len = stack.len();
    if len < 2 {
        return Err("stack underflow: OVER requires at least 2 items".to_string());
    }
    stack.push(stack[len - 2].clone());
    Ok(())
}

pub(crate) fn pick<T: Clone>(stack: &mut Vec<T>, index: i64) -> Result<(), String> {
    pick_n(stack, non_negative_index("PICK", index)?)
}

pub(crate) fn pick_n<T: Clone>(stack: &mut Vec<T>, index: usize) -> Result<(), String> {
    let len = stack.len();
    if index >= len {
        return Err(alloc::format!("PICK: index {index} out of range"));
    }
    stack.push(stack[len - 1 - index].clone());
    Ok(())
}

pub(crate) fn tuck<T: Clone>(stack: &mut Vec<T>) -> Result<(), String> {
    let len = stack.len();
    if len < 2 {
        return Err("stack underflow: TUCK requires at least 2 items".to_string());
    }
    let top = stack[len - 1].clone();
    stack.insert(len - 2, top);
    Ok(())
}

pub(crate) fn rot<T>(stack: &mut Vec<T>) -> Result<(), String> {
    let len = stack.len();
    if len < 3 {
        return Err("stack underflow: ROT requires at least 3 items".to_string());
    }
    let value = stack.remove(len - 3);
    stack.push(value);
    Ok(())
}

pub(crate) fn roll<T>(stack: &mut Vec<T>, index: i64) -> Result<(), String> {
    let index = non_negative_index("ROLL", index)?;
    let len = stack.len();
    if index >= len {
        return Err(alloc::format!("ROLL: index {index} out of range"));
    }
    let value = stack.remove(len - 1 - index);
    stack.push(value);
    Ok(())
}

pub(crate) fn reverse_top<T>(stack: &mut [T], count: usize) -> Result<(), String> {
    let len = stack.len();
    if count > len {
        return Err(alloc::format!(
            "REVERSEN: count {count} exceeds stack depth"
        ));
    }
    if count > 1 {
        stack[len - count..].reverse();
    }
    Ok(())
}

pub(crate) fn reverse_n<T>(stack: &mut [T], count: i64) -> Result<(), String> {
    if count < 0 {
        return Err("REVERSEN: negative count".to_string());
    }
    #[allow(clippy::cast_sign_loss)]
    reverse_top(stack, count as usize)
}

pub(crate) fn clear<T>(stack: &mut Vec<T>) {
    stack.clear();
}

fn non_negative_index(opcode: &str, index: i64) -> Result<usize, String> {
    if index < 0 {
        return Err(alloc::format!("{opcode}: negative index"));
    }
    #[allow(clippy::cast_sign_loss)]
    Ok(index as usize)
}
