//! JSON Test Runner for Neo VM
//!
//! This module provides functionality to execute JSON-based VM tests
//! that match the C# Neo.VM.Tests JSON test format exactly.

use neo_vm::stack_item::StackItem;
use neo_vm::{ExecutionEngine, Script};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// VM Unit Test structure (matches C# VMUT class)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VMUT {
    pub category: String,
    pub name: String,
    pub tests: Vec<VMUTTest>,
}

/// Individual test case (matches C# VMUTTest class)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VMUTTest {
    pub name: String,
    pub script: Vec<String>,
    pub steps: Vec<VMUTStep>,
}

/// Test step (matches C# VMUTStep class)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VMUTStep {
    pub actions: Vec<String>,
    pub result: VMUTExecutionEngineState,
}

/// Execution engine state (matches C# VMUTExecutionEngineState class)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VMUTExecutionEngineState {
    pub state: String,
    #[serde(rename = "invocationStack")]
    pub invocation_stack: Option<Vec<VMUTExecutionContext>>,
    #[serde(rename = "resultStack")]
    pub result_stack: Option<Vec<VMUTStackItem>>,
}

/// Execution context (matches C# VMUTExecutionContext class)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VMUTExecutionContext {
    #[serde(rename = "instructionPointer")]
    pub instruction_pointer: usize,
    #[serde(rename = "nextInstruction")]
    pub next_instruction: Option<String>,
    #[serde(rename = "evaluationStack")]
    pub evaluation_stack: Vec<VMUTStackItem>,
}

/// Stack item representation (matches C# VMUTStackItem class)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VMUTStackItem {
    #[serde(rename = "type")]
    pub item_type: String,
    pub value: Option<serde_json::Value>,
}

/// JSON Test Runner (matches C# VMJsonTestBase functionality)
pub struct JsonVmTestRunner {
    engine: ExecutionEngine,
}

impl Default for JsonVmTestRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonVmTestRunner {
    /// Create a new JSON test runner
    pub fn new() -> Self {
        Self {
            engine: ExecutionEngine::new(None),
        }
    }

    /// Execute a JSON test file (matches C# TestJson method)
    pub fn test_json_file<P: AsRef<Path>>(
        &mut self,
        file_path: P,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let file_content = fs::read_to_string(file_path.as_ref())?;
        let vmut: VMUT = serde_json::from_str(&file_content)?;

        println!("Testing: {} - {}", vmut.category, vmut.name);

        for test in vmut.tests {
            println!("  Running test: {}", test.name);
            self.execute_test(&test)?;
        }

        Ok(())
    }

    /// Execute a directory of JSON tests (matches C# TestJson method)
    pub fn test_json_directory<P: AsRef<Path>>(
        &mut self,
        dir_path: P,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let entries = fs::read_dir(dir_path)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
                println!("Processing file: {:?}", path);
                self.test_json_file(&path)?;
            } else if path.is_dir() {
                self.test_json_directory(&path)?;
            }
        }

        Ok(())
    }

    /// Execute a single test (matches C# ExecuteTest method)
    fn execute_test(&mut self, test: &VMUTTest) -> Result<(), Box<dyn std::error::Error>> {
        let script_bytes = self.compile_script(&test.script)?;
        let script = Script::new(script_bytes, false).unwrap();

        // Load script into engine
        self.engine.load_script(script, -1, 0).unwrap();

        // Execute each step
        for (step_index, step) in test.steps.iter().enumerate() {
            println!("    Step {}: {:?}", step_index + 1, step.actions);

            // Execute actions
            for action in &step.actions {
                self.execute_action(action)?;
            }

            self.verify_result(&step.result)?;
        }

        Ok(())
    }

    /// Compile script from opcode strings to bytes (matches C# script compilation)
    fn compile_script(&self, opcodes: &[String]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut bytes = Vec::new();

        for opcode in opcodes {
            match opcode.as_str() {
                "PUSHNULL" => bytes.push(0xf0),
                "PUSHDATA1" => bytes.push(0x0c),
                "PUSHDATA2" => bytes.push(0x0d),
                "PUSHDATA4" => bytes.push(0x0e),
                "PUSHM1" => bytes.push(0x0f),
                "PUSH0" => bytes.push(0x10),
                "PUSH1" => bytes.push(0x11),
                "PUSH2" => bytes.push(0x12),
                "PUSH3" => bytes.push(0x13),
                "PUSH4" => bytes.push(0x14),
                "PUSH5" => bytes.push(0x15),
                "PUSH6" => bytes.push(0x16),
                "PUSH7" => bytes.push(0x17),
                "PUSH8" => bytes.push(0x18),
                "PUSH9" => bytes.push(0x19),
                "PUSH10" => bytes.push(0x1a),
                "PUSH11" => bytes.push(0x1b),
                "PUSH12" => bytes.push(0x1c),
                "PUSH13" => bytes.push(0x1d),
                "PUSH14" => bytes.push(0x1e),
                "PUSH15" => bytes.push(0x1f),
                "PUSH16" => bytes.push(0x20),
                "NOP" => bytes.push(0x21),
                "JMP" => bytes.push(0x22),
                "JMPIF" => bytes.push(0x23),
                "JMPIFNOT" => bytes.push(0x24),
                "JMPEQ" => bytes.push(0x25),
                "JMPNE" => bytes.push(0x26),
                "JMPGT" => bytes.push(0x27),
                "JMPGE" => bytes.push(0x28),
                "JMPLT" => bytes.push(0x29),
                "JMPLE" => bytes.push(0x2a),
                "CALL" => bytes.push(0x2b),
                "CALLA" => bytes.push(0x2c),
                "CALLT" => bytes.push(0x2d),
                "ABORT" => bytes.push(0x2e),
                "ASSERT" => bytes.push(0x2f),
                "THROW" => bytes.push(0x3a),
                "TRY" => bytes.push(0x3b),
                "TryL" => bytes.push(0x3c),
                "ENDTRY" => bytes.push(0x3d),
                "EndtryL" => bytes.push(0x3e),
                "ENDFINALLY" => bytes.push(0x3f),
                "RET" => bytes.push(0x40),
                "SYSCALL" => bytes.push(0x41),
                "DEPTH" => bytes.push(0x43),
                "DROP" => bytes.push(0x45),
                "NIP" => bytes.push(0x46),
                "XDROP" => bytes.push(0x48),
                "CLEAR" => bytes.push(0x49),
                "DUP" => bytes.push(0x4a),
                "OVER" => bytes.push(0x4b),
                "PICK" => bytes.push(0x4d),
                "TUCK" => bytes.push(0x4e),
                "SWAP" => bytes.push(0x50),
                "ROT" => bytes.push(0x51),
                "ROLL" => bytes.push(0x52),
                "REVERSE3" => bytes.push(0x53),
                "REVERSE4" => bytes.push(0x54),
                "REVERSEN" => bytes.push(0x55),
                "INITSSLOT" => bytes.push(0x56),
                "INITSLOT" => bytes.push(0x57),
                "LDSFLD0" => bytes.push(0x58),
                "LDSFLD1" => bytes.push(0x59),
                "LDSFLD2" => bytes.push(0x5a),
                "LDSFLD3" => bytes.push(0x5b),
                "LDSFLD4" => bytes.push(0x5c),
                "LDSFLD5" => bytes.push(0x5d),
                "LDSFLD6" => bytes.push(0x5e),
                "LDSFLD" => bytes.push(0x5f),
                "STSFLD0" => bytes.push(0x60),
                "STSFLD1" => bytes.push(0x61),
                "STSFLD2" => bytes.push(0x62),
                "STSFLD3" => bytes.push(0x63),
                "STSFLD4" => bytes.push(0x64),
                "STSFLD5" => bytes.push(0x65),
                "STSFLD6" => bytes.push(0x66),
                "STSFLD" => bytes.push(0x67),
                "LDLOC0" => bytes.push(0x68),
                "LDLOC1" => bytes.push(0x69),
                "LDLOC2" => bytes.push(0x6a),
                "LDLOC3" => bytes.push(0x6b),
                "LDLOC4" => bytes.push(0x6c),
                "LDLOC5" => bytes.push(0x6d),
                "LDLOC6" => bytes.push(0x6e),
                "LDLOC" => bytes.push(0x6f),
                "STLOC0" => bytes.push(0x70),
                "STLOC1" => bytes.push(0x71),
                "STLOC2" => bytes.push(0x72),
                "STLOC3" => bytes.push(0x73),
                "STLOC4" => bytes.push(0x74),
                "STLOC5" => bytes.push(0x75),
                "STLOC6" => bytes.push(0x76),
                "STLOC" => bytes.push(0x77),
                "LDARG0" => bytes.push(0x78),
                "LDARG1" => bytes.push(0x79),
                "LDARG2" => bytes.push(0x7a),
                "LDARG3" => bytes.push(0x7b),
                "LDARG4" => bytes.push(0x7c),
                "LDARG5" => bytes.push(0x7d),
                "LDARG6" => bytes.push(0x7e),
                "LDARG" => bytes.push(0x7f),
                "STARG0" => bytes.push(0x80),
                "STARG1" => bytes.push(0x81),
                "STARG2" => bytes.push(0x82),
                "STARG3" => bytes.push(0x83),
                "STARG4" => bytes.push(0x84),
                "STARG5" => bytes.push(0x85),
                "STARG6" => bytes.push(0x86),
                "STARG" => bytes.push(0x87),
                _ => return Err(format!("Unknown opcode: {}", opcode).into()),
            }
        }

        if bytes.is_empty() || bytes[bytes.len() - 1] != 0x40 {
            bytes.push(0x40); // RET
        }

        Ok(bytes)
    }

    /// Execute an action (matches C# action execution)
    fn execute_action(&mut self, action: &str) -> Result<(), Box<dyn std::error::Error>> {
        match action {
            "stepInto" => {
                self.engine.execute_next_instruction()?;
            }
            "stepOver" => {
                self.engine.execute_next_instruction()?;
            }
            "stepOut" => {
                while !self.engine.state().is_halt() && !self.engine.state().is_fault() {
                    self.engine.execute_next_instruction()?;
                }
            }
            _ => return Err(format!("Unknown action: {}", action).into()),
        }
        Ok(())
    }

    /// Verify the result matches expected state (matches C# result verification)
    fn verify_result(
        &self,
        expected: &VMUTExecutionEngineState,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Verify VM state
        let state = self.engine.state();
        let actual_state = if state.is_fault() {
            "FAULT"
        } else if state.is_halt() {
            "HALT"
        } else if state.is_break() {
            "BREAK"
        } else {
            "NONE"
        };

        if actual_state != expected.state {
            return Err(format!(
                "State mismatch: expected {}, got {}",
                expected.state, actual_state
            )
            .into());
        }

        if let Some(expected_invocation_stack) = &expected.invocation_stack {
            self.verify_invocation_stack(expected_invocation_stack)?;
        }

        if let Some(expected_result_stack) = &expected.result_stack {
            self.verify_result_stack(expected_result_stack)?;
        }

        Ok(())
    }

    /// Verify invocation stack matches expected state (matches C# verification)
    fn verify_invocation_stack(
        &self,
        expected: &[VMUTExecutionContext],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let actual = self.engine.invocation_stack();
        if actual.len() != expected.len() {
            return Err(format!(
                "Invocation stack length mismatch: expected {}, got {}",
                expected.len(),
                actual.len()
            )
            .into());
        }

        for (depth, expected_ctx) in expected.iter().enumerate() {
            let actual_ctx = &actual[actual.len().saturating_sub(depth + 1)];

            let actual_ip = actual_ctx.instruction_pointer();
            if actual_ip != expected_ctx.instruction_pointer {
                return Err(format!(
                    "Invocation stack[{}] instruction pointer mismatch: expected {}, got {}",
                    depth, expected_ctx.instruction_pointer, actual_ip
                )
                .into());
            }

            if let Some(expected_next) = &expected_ctx.next_instruction {
                let actual_next = match actual_ctx.current_instruction() {
                    Ok(instr) => format!("{:?}", instr.opcode()),
                    Err(_) => "<INVALID>".to_string(),
                };
                if actual_next != *expected_next {
                    return Err(format!(
                        "Invocation stack[{}] next instruction mismatch: expected {}, got {}",
                        depth, expected_next, actual_next
                    )
                    .into());
                }
            }

            verify_stack_against_evaluation(
                &expected_ctx.evaluation_stack,
                actual_ctx.evaluation_stack(),
            )
            .map_err(|e| format!("Invocation stack[{}] evaluation stack: {}", depth, e))?;
        }

        Ok(())
    }

    /// Verify result stack matches expected state (matches C# verification)
    fn verify_result_stack(
        &self,
        expected: &[VMUTStackItem],
    ) -> Result<(), Box<dyn std::error::Error>> {
        verify_stack_against_evaluation(expected, self.engine.result_stack())
            .map_err(|e| format!("Result stack: {}", e))?;
        Ok(())
    }
}

fn verify_stack_against_evaluation(
    expected: &[VMUTStackItem],
    actual: &neo_vm::evaluation_stack::EvaluationStack,
) -> Result<(), String> {
    if actual.len() != expected.len() {
        return Err(format!(
            "length mismatch: expected {}, got {}",
            expected.len(),
            actual.len()
        ));
    }

    for (idx, expected_item) in expected.iter().enumerate() {
        let actual_item = actual
            .peek(idx)
            .map_err(|e| format!("peek({idx}) failed: {e}"))?;
        verify_stack_item(expected_item, actual_item)
            .map_err(|e| format!("item[{idx}] mismatch: {e}"))?;
    }
    Ok(())
}

fn verify_stack_item(expected: &VMUTStackItem, actual: &StackItem) -> Result<(), String> {
    let expected_type = expected.item_type.to_ascii_lowercase();
    match expected_type.as_str() {
        "null" => match actual {
            StackItem::Null => Ok(()),
            _ => Err(format!("expected null, got {:?}", actual.stack_item_type())),
        },
        "boolean" => match actual {
            StackItem::Boolean(actual_bool) => {
                let expected_bool = expected
                    .value
                    .as_ref()
                    .and_then(|v| v.as_bool())
                    .ok_or_else(|| "expected boolean value".to_string())?;
                if *actual_bool == expected_bool {
                    Ok(())
                } else {
                    Err(format!("expected {}, got {}", expected_bool, actual_bool))
                }
            }
            _ => Err(format!(
                "expected boolean, got {:?}",
                actual.stack_item_type()
            )),
        },
        "integer" => match actual {
            StackItem::Integer(actual_int) => {
                let expected_int = parse_bigint(expected.value.as_ref())
                    .ok_or_else(|| "expected integer value".to_string())?;
                if *actual_int == expected_int {
                    Ok(())
                } else {
                    Err(format!("expected {}, got {}", expected_int, actual_int))
                }
            }
            _ => Err(format!(
                "expected integer, got {:?}",
                actual.stack_item_type()
            )),
        },
        "bytestring" => match actual {
            StackItem::ByteString(bytes) => {
                let expected_hex = expected
                    .value
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "expected bytestring hex value".to_string())?;
                let actual_hex = bytes_to_hex_prefixed(bytes);
                if hex_eq(expected_hex, &actual_hex) {
                    Ok(())
                } else {
                    Err(format!("expected {}, got {}", expected_hex, actual_hex))
                }
            }
            _ => Err(format!(
                "expected ByteString, got {:?}",
                actual.stack_item_type()
            )),
        },
        "buffer" => match actual {
            StackItem::Buffer(buffer) => {
                let expected_hex = expected
                    .value
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "expected buffer hex value".to_string())?;
                let actual_hex = bytes_to_hex_prefixed(&buffer.data());
                if hex_eq(expected_hex, &actual_hex) {
                    Ok(())
                } else {
                    Err(format!("expected {}, got {}", expected_hex, actual_hex))
                }
            }
            _ => Err(format!(
                "expected Buffer, got {:?}",
                actual.stack_item_type()
            )),
        },
        "pointer" => match actual {
            StackItem::Pointer(pointer) => {
                let expected_ptr = parse_bigint(expected.value.as_ref())
                    .ok_or_else(|| "expected pointer value".to_string())?;
                let actual_ptr = BigInt::from(pointer.position());
                if actual_ptr == expected_ptr {
                    Ok(())
                } else {
                    Err(format!("expected {}, got {}", expected_ptr, actual_ptr))
                }
            }
            _ => Err(format!(
                "expected Pointer, got {:?}",
                actual.stack_item_type()
            )),
        },
        "array" => match actual {
            StackItem::Array(array) => {
                let expected_items = expected
                    .value
                    .as_ref()
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| "expected array value".to_string())?;
                if array.len() != expected_items.len() {
                    return Err(format!(
                        "array length mismatch: expected {}, got {}",
                        expected_items.len(),
                        array.len()
                    ));
                }
                for (idx, expected_child) in expected_items.iter().enumerate() {
                    let expected_child: VMUTStackItem =
                        serde_json::from_value(expected_child.clone())
                            .map_err(|e| format!("invalid array element[{idx}] json: {e}"))?;
                    let actual_child = array
                        .get(idx)
                        .ok_or_else(|| format!("missing array element[{idx}]"))?;
                    verify_stack_item(&expected_child, &actual_child)
                        .map_err(|e| format!("array element[{idx}]: {e}"))?;
                }
                Ok(())
            }
            _ => Err(format!(
                "expected Array, got {:?}",
                actual.stack_item_type()
            )),
        },
        "struct" => match actual {
            StackItem::Struct(structure) => {
                let expected_items = expected
                    .value
                    .as_ref()
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| "expected struct value".to_string())?;
                if structure.len() != expected_items.len() {
                    return Err(format!(
                        "struct length mismatch: expected {}, got {}",
                        expected_items.len(),
                        structure.len()
                    ));
                }
                for (idx, expected_child) in expected_items.iter().enumerate() {
                    let expected_child: VMUTStackItem =
                        serde_json::from_value(expected_child.clone())
                            .map_err(|e| format!("invalid struct element[{idx}] json: {e}"))?;
                    let actual_child = structure
                        .get(idx)
                        .map_err(|e| format!("missing struct element[{idx}]: {e}"))?;
                    verify_stack_item(&expected_child, &actual_child)
                        .map_err(|e| format!("struct element[{idx}]: {e}"))?;
                }
                Ok(())
            }
            _ => Err(format!(
                "expected Struct, got {:?}",
                actual.stack_item_type()
            )),
        },
        "map" => match actual {
            StackItem::Map(map) => {
                let expected_obj = expected
                    .value
                    .as_ref()
                    .and_then(|v| v.as_object())
                    .ok_or_else(|| "expected map value".to_string())?;

                if map.items().len() != expected_obj.len() {
                    return Err(format!(
                        "map size mismatch: expected {}, got {}",
                        expected_obj.len(),
                        map.items().len()
                    ));
                }

                let mut actual_by_key: HashMap<String, StackItem> = HashMap::new();
                for (key, value) in map.items().iter() {
                    let key_str = map_key_string(key)?;
                    actual_by_key.insert(key_str, value.clone());
                }

                for (expected_key, expected_value) in expected_obj.iter() {
                    let expected_child: VMUTStackItem =
                        serde_json::from_value(expected_value.clone()).map_err(|e| {
                            format!("invalid map value for key {expected_key}: {e}")
                        })?;
                    let actual_value = actual_by_key
                        .get(expected_key)
                        .ok_or_else(|| format!("missing map key {}", expected_key))?;
                    verify_stack_item(&expected_child, actual_value)
                        .map_err(|e| format!("map key {expected_key}: {e}"))?;
                }

                Ok(())
            }
            _ => Err(format!("expected Map, got {:?}", actual.stack_item_type())),
        },
        "interop" => match actual {
            StackItem::InteropInterface(_) => {
                if expected.value.is_none() {
                    return Ok(());
                }
                let expected_value = expected
                    .value
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .unwrap_or("Object");
                if expected_value == "Object" {
                    Ok(())
                } else {
                    Err(format!("expected interop {}, got Object", expected_value))
                }
            }
            _ => Err(format!(
                "expected InteropInterface, got {:?}",
                actual.stack_item_type()
            )),
        },
        other => Err(format!("unsupported expected stack item type: {}", other)),
    }
}

fn parse_bigint(value: Option<&Value>) -> Option<BigInt> {
    match value {
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                Some(BigInt::from(i))
            } else {
                n.as_u64().map(BigInt::from)
            }
        }
        Some(Value::String(s)) => s.parse::<BigInt>().ok(),
        _ => None,
    }
}

fn map_key_string(key: &StackItem) -> Result<String, String> {
    let bytes = key
        .as_bytes()
        .map_err(|e| format!("failed to encode map key: {e}"))?;
    if bytes.is_empty() {
        return Ok(String::new());
    }
    Ok(bytes_to_hex_prefixed(&bytes))
}

fn bytes_to_hex_prefixed(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(2 + bytes.len() * 2);
    out.push_str("0x");
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn hex_eq(expected: &str, actual: &str) -> bool {
    expected.trim().eq_ignore_ascii_case(actual.trim())
}

const HEX: &[u8; 16] = b"0123456789abcdef";

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn test_json_runner_creation() {
        let _runner = JsonVmTestRunner::new();
    }

    #[test]
    fn test_script_compilation() {
        let runner = JsonVmTestRunner::new();
        let opcodes = vec!["PUSHNULL".to_string(), "RET".to_string()];
        let result = runner.compile_script(&opcodes).unwrap();
        assert_eq!(result, vec![0xf0, 0x40]); // PUSHNULL + RET
    }
}
