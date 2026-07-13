//! Strict VM script validation helpers re-exported from `neo-vm`.

pub use neo_vm::{
    ScriptInstruction, ValidatedScript, ValidationResult, parse_script_instructions,
    validate_script, validate_strict_script,
};
