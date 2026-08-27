use super::super::state::{PendingException, TryFrame, TryStack};
use alloc::string::{String, ToString};

pub(super) enum Dispatch {
    Fallthrough,
    Continue,
}

pub(super) fn enter_try_short(
    script: &[u8],
    ip: usize,
    call_depth: usize,
    try_frames: &mut TryStack,
    pending_error: &mut Option<PendingException>,
) -> Result<usize, String> {
    if ip + 3 > script.len() {
        *pending_error = Some(PendingException::message(
            "truncated TRY operand".to_string(),
        ));
        return Ok(ip);
    }
    let catch_offset = script[ip + 1] as i8;
    let finally_offset = script[ip + 2] as i8;
    let catch_ip = if catch_offset != 0 {
        (ip as isize + catch_offset as isize) as usize
    } else {
        0
    };
    let finally_ip = if finally_offset != 0 {
        (ip as isize + finally_offset as isize) as usize
    } else {
        0
    };
    if catch_ip > script.len() || finally_ip > script.len() {
        *pending_error = Some(PendingException::message(
            "TRY target out of bounds".to_string(),
        ));
        return Ok(ip);
    }
    try_frames.push(TryFrame {
        catch_ip,
        finally_ip,
        call_depth,
        caught: false,
        in_finally: false,
        end_ip: 0,
    })?;
    Ok(ip + 3)
}

pub(super) fn enter_try_long(
    script: &[u8],
    ip: usize,
    call_depth: usize,
    try_frames: &mut TryStack,
    pending_error: &mut Option<PendingException>,
) -> Result<usize, String> {
    if ip + 9 > script.len() {
        *pending_error = Some(PendingException::message(
            "truncated TRY_L operand".to_string(),
        ));
        return Ok(ip);
    }
    let catch_offset = i32::from_le_bytes([
        script[ip + 1],
        script[ip + 2],
        script[ip + 3],
        script[ip + 4],
    ]);
    let finally_offset = i32::from_le_bytes([
        script[ip + 5],
        script[ip + 6],
        script[ip + 7],
        script[ip + 8],
    ]);
    let catch_ip = if catch_offset != 0 {
        (ip as isize + catch_offset as isize) as usize
    } else {
        0
    };
    let finally_ip = if finally_offset != 0 {
        (ip as isize + finally_offset as isize) as usize
    } else {
        0
    };
    if catch_ip > script.len() || finally_ip > script.len() {
        *pending_error = Some(PendingException::message(
            "TRY_L target out of bounds".to_string(),
        ));
        return Ok(ip);
    }
    try_frames.push(TryFrame {
        catch_ip,
        finally_ip,
        call_depth,
        caught: false,
        in_finally: false,
        end_ip: 0,
    })?;
    Ok(ip + 9)
}

pub(super) fn end_try_short(
    script: &[u8],
    ip: usize,
    try_frames: &mut TryStack,
    pending_error: &mut Option<PendingException>,
) -> usize {
    if ip + 2 > script.len() {
        *pending_error = Some(PendingException::message(
            "truncated ENDTRY operand".to_string(),
        ));
        return ip;
    }
    let offset = script[ip + 1] as i8;
    let target_ip = if offset != 0 {
        (ip as isize + offset as isize) as usize
    } else {
        ip + 2
    };
    end_try_at(target_ip, try_frames)
}

pub(super) fn end_try_long(
    script: &[u8],
    ip: usize,
    try_frames: &mut TryStack,
    pending_error: &mut Option<PendingException>,
) -> usize {
    if ip + 5 > script.len() {
        *pending_error = Some(PendingException::message(
            "truncated ENDTRY_L operand".to_string(),
        ));
        return ip;
    }
    let offset = i32::from_le_bytes([
        script[ip + 1],
        script[ip + 2],
        script[ip + 3],
        script[ip + 4],
    ]);
    let target_ip = if offset != 0 {
        (ip as isize + offset as isize) as usize
    } else {
        ip + 5
    };
    end_try_at(target_ip, try_frames)
}

pub(super) fn end_finally(
    ip: usize,
    try_frames: &mut TryStack,
    pending_error: &Option<PendingException>,
) -> Result<Option<usize>, String> {
    let in_finally = try_frames.last_mut().is_some_and(|frame| frame.in_finally);
    if !in_finally {
        return Err("ENDFINALLY without matching finally context".to_string());
    }

    let frame = try_frames.pop().expect("frame checked above");
    if pending_error.is_some() {
        return Ok(Some(ip));
    }
    if frame.end_ip != 0 {
        return Ok(Some(frame.end_ip));
    }
    Ok(None)
}

fn end_try_at(target_ip: usize, try_frames: &mut TryStack) -> usize {
    if let Some(frame) = try_frames.last_mut() {
        if frame.finally_ip != 0 && !frame.in_finally {
            frame.end_ip = target_ip;
            frame.in_finally = true;
            return frame.finally_ip;
        }
    }
    try_frames.pop();
    target_ip
}
