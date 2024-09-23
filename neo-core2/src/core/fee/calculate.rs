use crate::io;
use crate::vm;
use crate::vm::emit;
use crate::vm::opcode;

// ECDSAVerifyPrice is a gas price of a single verification.
const ECDSA_VERIFY_PRICE: i64 = 1 << 15;

// Calculate returns network fee for a transaction.
pub fn calculate(base: i64, script: &[u8]) -> (i64, usize) {
    let mut net_fee: i64 = 0;
    let mut size: usize = 0;

    if vm::is_signature_contract(script) {
        size += 67 + io::get_var_size(script);
        net_fee += opcode(base, opcode::PUSHDATA1, opcode::PUSHDATA1) + base * ECDSA_VERIFY_PRICE;
    } else if let Some((m, pubs)) = vm::parse_multi_sig_contract(script) {
        let n = pubs.len();
        let size_inv = 66 * m;
        size += io::get_var_size(size_inv) + size_inv + io::get_var_size(script);
        net_fee += calculate_multisig(base, m) + calculate_multisig(base, n);
        net_fee += base * ECDSA_VERIFY_PRICE * n as i64;
    } /*else {
        // We can support more contract types in the future.
    }*/
    (net_fee, size)
}

fn calculate_multisig(base: i64, n: usize) -> i64 {
    let mut result = opcode(base, opcode::PUSHDATA1) * n as i64;
    let mut bw = io::BufBinWriter::new();
    emit::int(&mut bw, n as i64);
    // it's a hack because coefficients of small PUSH* opcodes are equal
    result += opcode(base, opcode::Opcode(bw.bytes()[0]));
    result
}
