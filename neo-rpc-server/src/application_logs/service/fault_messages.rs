use super::*;
/// C# `OpCode.ASSERT` uses `ASSERT is executed with false result.` Rust's
/// `AssertFailed` display omits the period. C# `CallContract` reports a missing
/// method as `Method "name" with N parameter(s) doesn't exist in the contract 0xhash.`
/// Rust's VM display wraps that fault in `Interop service error` and
/// `Invalid operation`, and `UInt160`'s debug text adds a type name.
pub(crate) fn csharp_fault_exception_message(message: &str) -> String {
    let stripped = strip_vm_diagnostic_suffix(message);
    if is_engine_gas_fault(stripped) {
        "Insufficient GAS.".to_string()
    } else if stripped == "ASSERT is executed with false result" {
        "ASSERT is executed with false result.".to_string()
    } else if let Some(missing_method) = csharp_missing_contract_method(stripped) {
        missing_method
    } else {
        stripped.to_string()
    }
}

fn csharp_missing_contract_method(message: &str) -> Option<String> {
    let message = unwrap_interop_service_error(message);
    let message = message
        .strip_prefix("Invalid operation: ")
        .unwrap_or(message);
    let rest = message.strip_prefix("Method ")?;
    let mut chars = rest.chars();
    let quote = chars.next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let rest = chars.as_str();
    let end = rest.find(quote)?;
    let name = &rest[..end];
    if name.is_empty() {
        return None;
    }
    let rest = rest[end + quote.len_utf8()..].strip_prefix(" with ")?;
    let count_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if count_len == 0 {
        return None;
    }
    let count = &rest[..count_len];
    let rest = rest[count_len..].strip_prefix(" parameter(s) doesn't exist in the contract ")?;
    let (hash, tail) = contract_hash_text(rest)?;
    if tail != "." {
        return None;
    }
    Some(format!(
        "Method \"{name}\" with {count} parameter(s) doesn't exist in the contract {hash}."
    ))
}

fn unwrap_interop_service_error(message: &str) -> &str {
    const PREFIX: &str = "Interop service error: service ";
    const MARKER: &str = ", error: ";
    let Some(rest) = message.strip_prefix(PREFIX) else {
        return message;
    };
    let Some(index) = rest.find(MARKER) else {
        return message;
    };
    if rest[..index].is_empty() {
        return message;
    }
    let inner = &rest[index + MARKER.len()..];
    if inner.is_empty() { message } else { inner }
}

fn contract_hash_text(text: &str) -> Option<(&str, &str)> {
    if let Some(inner) = text.strip_prefix("UInt160(") {
        let end = inner.find(')')?;
        let hash = &inner[..end];
        if !is_script_hash_text(hash) {
            return None;
        }
        return Some((hash, &inner[end + 1..]));
    }
    const HASH_LEN: usize = 42;
    if text.len() < HASH_LEN || !is_script_hash_text(&text[..HASH_LEN]) {
        return None;
    }
    Some((&text[..HASH_LEN], &text[HASH_LEN..]))
}

fn is_script_hash_text(text: &str) -> bool {
    let Some(hex) = text.strip_prefix("0x") else {
        return false;
    };
    hex.len() == 40 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn strip_vm_diagnostic_suffix(message: &str) -> &str {
    let Some(start) = message.rfind(" [ip=") else {
        return message;
    };
    let suffix = &message[start..];
    if suffix_is_vm_diagnostic(suffix) {
        &message[..start]
    } else {
        message
    }
}

fn suffix_is_vm_diagnostic(suffix: &str) -> bool {
    let Some(rest) = suffix.strip_prefix(" [ip=") else {
        return false;
    };
    let Some(rest) = rest.strip_suffix(']') else {
        return false;
    };
    let mut parts = rest.split(' ');
    let Some(ip) = parts.next() else {
        return false;
    };
    if ip.is_empty() || !ip.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    match (parts.next(), parts.next(), parts.next()) {
        (None, None, None) => true,
        (Some(opcode), Some(depth), None) => {
            let Some(opcode_name) = opcode.strip_prefix("opcode=") else {
                return false;
            };
            let Some(depth) = depth.strip_prefix("eval_depth=") else {
                return false;
            };
            !opcode_name.is_empty()
                && !depth.is_empty()
                && depth.bytes().all(|byte| byte.is_ascii_digit())
        }
        _ => false,
    }
}

fn is_engine_gas_fault(message: &str) -> bool {
    matches!(
        message,
        "Gas limit exceeded" | "Out of gas" | "VM exceeded gas limit during execution"
    ) || is_counted_gas_fault(message, "Gas exhausted: used ", ", limit ")
        || is_counted_gas_fault(message, "Insufficient gas: required ", ", available ")
}

fn is_counted_gas_fault(message: &str, prefix: &str, separator: &str) -> bool {
    let Some(rest) = message.strip_prefix(prefix) else {
        return false;
    };
    let Some((left, right)) = rest.split_once(separator) else {
        return false;
    };
    !left.is_empty()
        && left.bytes().all(|byte| byte.is_ascii_digit())
        && !right.is_empty()
        && right.bytes().all(|byte| byte.is_ascii_digit())
}

pub(super) fn normalize_stored_fault_exceptions(value: &mut Value) -> bool {
    let Some(executions) = value.get_mut("executions").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for exec in executions {
        let Some(obj) = exec.as_object_mut() else {
            continue;
        };
        let Some(message) = obj.get("exception").and_then(Value::as_str) else {
            continue;
        };
        let normalized = csharp_fault_exception_message(message);
        if normalized != message {
            obj.insert("exception".to_string(), Value::String(normalized));
            changed = true;
        }
    }
    changed
}

pub(super) fn promote_native_persist_vmstate(value: &mut Value) -> bool {
    let Some(executions) = value.get_mut("executions").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for exec in executions {
        let Some(obj) = exec.as_object_mut() else {
            continue;
        };
        let trigger = obj.get("trigger").and_then(Value::as_str);
        let vmstate = obj.get("vmstate").and_then(Value::as_str);
        if matches!(trigger, Some("OnPersist" | "PostPersist")) && vmstate == Some("BREAK") {
            obj.insert("vmstate".to_string(), Value::String("HALT".to_string()));
            changed = true;
        }
    }
    changed
}
