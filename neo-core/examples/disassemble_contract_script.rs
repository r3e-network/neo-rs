use base64::Engine as _;
use neo_vm::op_code::OpCode;
use neo_vm::Script;

fn format_ascii_preview(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for byte in bytes.iter().copied() {
        if byte.is_ascii_graphic() || byte == b' ' {
            out.push(byte as char);
        } else {
            out.push('.');
        }
    }
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let script_b64 = args.next().ok_or(
        "usage: disassemble_contract_script <base64_script> [start_offset] [instruction_limit]",
    )?;
    let start_offset = args
        .next()
        .map(|raw| raw.parse::<usize>())
        .transpose()?
        .unwrap_or(0);
    let instruction_limit = args
        .next()
        .map(|raw| raw.parse::<usize>())
        .transpose()?
        .unwrap_or(120);

    let script_bytes = base64::engine::general_purpose::STANDARD.decode(script_b64)?;
    let script = Script::new(script_bytes.clone(), false)?;

    println!(
        "script_len={} start_offset={} instruction_limit={}",
        script_bytes.len(),
        start_offset,
        instruction_limit
    );

    let mut offset = start_offset;
    let mut emitted = 0usize;
    while offset < script.len() && emitted < instruction_limit {
        let instruction = script
            .get_instruction(offset)
            .map_err(|err| format!("failed to decode instruction at {offset}: {err}"))?;
        let opcode = instruction.opcode();
        let size = instruction.size();

        match opcode {
            OpCode::SYSCALL => {
                println!(
                    "offset={offset:>5} opcode={opcode:?} size={size} syscall=0x{:08x}",
                    instruction.token_u32()
                );
            }
            OpCode::CALLT => {
                println!(
                    "offset={offset:>5} opcode={opcode:?} size={size} token_id={}",
                    instruction.token_u16()
                );
            }
            OpCode::CALL
            | OpCode::CALL_L
            | OpCode::JMP
            | OpCode::JMP_L
            | OpCode::JMPIF
            | OpCode::JMPIF_L
            | OpCode::JMPIFNOT
            | OpCode::JMPIFNOT_L
            | OpCode::JMPEQ
            | OpCode::JMPEQ_L
            | OpCode::JMPNE
            | OpCode::JMPNE_L
            | OpCode::JMPGT
            | OpCode::JMPGT_L
            | OpCode::JMPGE
            | OpCode::JMPGE_L
            | OpCode::JMPLT
            | OpCode::JMPLT_L
            | OpCode::JMPLE
            | OpCode::JMPLE_L
            | OpCode::TRY
            | OpCode::TRY_L
            | OpCode::ENDTRY
            | OpCode::ENDTRY_L => {
                println!(
                    "offset={offset:>5} opcode={opcode:?} size={size} operand=0x{}",
                    hex::encode(instruction.operand())
                );
            }
            OpCode::PUSHDATA1 | OpCode::PUSHDATA2 | OpCode::PUSHDATA4 => {
                let payload = instruction.operand();
                let preview_len = payload.len().min(48);
                println!(
                    "offset={offset:>5} opcode={opcode:?} size={size} data_len={} preview_hex=0x{} preview_ascii=\"{}\"",
                    payload.len(),
                    hex::encode(&payload[..preview_len]),
                    format_ascii_preview(&payload[..preview_len]),
                );
            }
            _ => {
                println!("offset={offset:>5} opcode={opcode:?} size={size}");
            }
        }

        if size == 0 {
            return Err(format!("decoded zero-size instruction at offset {offset}").into());
        }
        offset += size;
        emitted += 1;
    }

    Ok(())
}
