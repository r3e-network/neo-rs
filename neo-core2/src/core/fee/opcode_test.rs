use test::Bencher;
use neo_core::vm::opcode::Opcode;

const FEE_FACTOR: u32 = 30;

// The most common Opcode() use case is to get price for a single opcode.
#[bench]
fn benchmark_opcode1(b: &mut Bencher) {
    // Just so that we don't always test the same opcode.
    let script = [Opcode::NOP, Opcode::ADD, Opcode::SYSCALL, Opcode::APPEND];
    let l = script.len();
    b.iter(|| {
        for n in 0..b.iterations() {
            let _ = opcode(FEE_FACTOR, script[n as usize % l]);
        }
    });
}
