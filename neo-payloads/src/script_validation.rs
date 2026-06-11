//! Strict VM script validation helpers re-exported from `neo-vm-rs`.

pub use neo_vm_rs::{
    parse_script_instructions, validate_script, validate_strict_script, ScriptInstruction,
    ValidatedScript, ValidationResult,
};
