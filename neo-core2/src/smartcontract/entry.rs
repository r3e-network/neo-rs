use std::fmt;
use neo_core2::core::interop::interopnames;
use neo_core2::io::BufBinWriter;
use neo_core2::smartcontract::callflag;
use neo_core2::util::Uint160;
use neo_core2::vm::emit;
use neo_core2::vm::opcode;

/// Creates a script that calls 'operation' method of the 'contract' with the specified arguments.
/// This method is expected to return an iterator that then is traversed (using iterator.Next) with
/// values (iterator.Value) extracted and added into array. At most maxIteratorResultItems number of
/// items is processed this way (and this number can't exceed VM limits), the result of the script
/// is an array containing extracted value elements. This script can be useful for interactions with
/// RPC server that have iterator sessions disabled.
pub fn create_call_and_unwrap_iterator_script(
    contract: Uint160,
    operation: &str,
    max_iterator_result_items: i32,
    params: &[impl Serialize],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut script = BufBinWriter::new();
    let (jmp_if_not_offset, jmp_if_max_reached_offset) = emit_call_and_unwrap_iterator_script(
        &mut script,
        contract,
        operation,
        max_iterator_result_items,
        params,
    )?;

    // End of the program: push the result on stack and return.
    let load_result_offset = script.len();
    emit::opcodes(&mut script.bin_writer, &[opcode::NIP, opcode::NIP])?;

    // Fill in JMPIFNOT instruction parameter.
    let mut bytes = script.into_vec();
    bytes[jmp_if_not_offset + 1] = (load_result_offset - jmp_if_not_offset) as u8;
    // Fill in jmpIfMaxReachedOffset instruction parameter.
    bytes[jmp_if_max_reached_offset + 1] = (load_result_offset - jmp_if_max_reached_offset) as u8;

    Ok(bytes)
}

/// Creates a script that calls 'operation' method of the 'contract' with the specified arguments.
/// This method is expected to return an array of the first iterator items (up to maxIteratorResultItems,
/// which cannot exceed VM limits) and, optionally, an iterator that then is traversed (using iterator.Next).
/// The result of the script is an array containing extracted value elements and an iterator, if it can contain more items.
/// If the iterator is present, it lies on top of the stack.
/// Note, however, that if an iterator is returned, the number of remaining items can still be 0.
/// This script should only be used for interactions with RPC server that have iterator sessions enabled.
pub fn create_call_and_prefetch_iterator_script(
    contract: Uint160,
    operation: &str,
    max_iterator_result_items: i32,
    params: &[impl Serialize],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut script = BufBinWriter::new();
    let (jmp_if_not_offset, jmp_if_max_reached_offset) = emit_call_and_unwrap_iterator_script(
        &mut script,
        contract,
        operation,
        max_iterator_result_items,
        params,
    )?;

    // 1st possibility: jump here when the maximum number of items was reached.
    let retain_iterator_offset = script.len();
    emit::opcodes(&mut script.bin_writer, &[opcode::ROT, opcode::DROP, opcode::SWAP, opcode::RET])?;

    // 2nd possibility: jump here when the iterator has no more items.
    let load_result_offset = script.len();
    emit::opcodes(&mut script.bin_writer, &[opcode::ROT, opcode::DROP, opcode::NIP])?;

    // Fill in JMPIFNOT instruction parameter.
    let mut bytes = script.into_vec();
    bytes[jmp_if_not_offset + 1] = (load_result_offset - jmp_if_not_offset) as u8;
    // Fill in jmpIfMaxReachedOffset instruction parameter.
    bytes[jmp_if_max_reached_offset + 1] = (retain_iterator_offset - jmp_if_max_reached_offset) as u8;

    Ok(bytes)
}

fn emit_call_and_unwrap_iterator_script(
    script: &mut BufBinWriter,
    contract: Uint160,
    operation: &str,
    max_iterator_result_items: i32,
    params: &[impl Serialize],
) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    emit::int(&mut script.bin_writer, max_iterator_result_items as i64)?;
    emit::app_call(&mut script.bin_writer, &contract, operation, callflag::All, params)?;
    emit::opcodes(&mut script.bin_writer, &[opcode::NEWARRAY0])?;

    // Start the iterator traversal cycle.
    let iterator_traverse_cycle_start_offset = script.len();
    emit::opcodes(&mut script.bin_writer, &[opcode::OVER])?;
    emit::syscall(&mut script.bin_writer, interopnames::SYSTEM_ITERATOR_NEXT)?;
    let jmp_if_not_offset = script.len();
    emit::instruction(&mut script.bin_writer, opcode::JMPIFNOT, &[0x00])?;
    emit::opcodes(&mut script.bin_writer, &[opcode::DUP, opcode::PUSH2, opcode::PICK])?;
    emit::syscall(&mut script.bin_writer, interopnames::SYSTEM_ITERATOR_VALUE)?;
    emit::opcodes(&mut script.bin_writer, &[opcode::APPEND, opcode::DUP, opcode::SIZE, opcode::PUSH3, opcode::PICK, opcode::GE])?;
    let jmp_if_max_reached_offset = script.len();
    emit::instruction(&mut script.bin_writer, opcode::JMPIF, &[0x00])?;
    let jmp_offset = script.len();
    emit::instruction(&mut script.bin_writer, opcode::JMP, &[(iterator_traverse_cycle_start_offset - jmp_offset) as u8])?;

    Ok((jmp_if_not_offset, jmp_if_max_reached_offset))
}

/// Returns a script that calls contract's method with the specified parameters.
/// Whatever this method returns remains on the stack.
pub fn create_call_script(
    contract: Uint160,
    method: &str,
    params: &[impl Serialize],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut builder = Builder::new();
    builder.invoke_method(contract, method, params)?;
    builder.script()
}

/// Returns a script that calls contract's method with the specified parameters
/// expecting a Boolean value to be returned that then is used for ASSERT.
pub fn create_call_with_assert_script(
    contract: Uint160,
    method: &str,
    params: &[impl Serialize],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut builder = Builder::new();
    builder.invoke_with_assert(contract, method, params)?;
    builder.script()
}
