//! Canonical, direct-native, and dual-execution persistence benchmarks.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use neo_native_contracts::native::NativeHelpers;
use neo_native_contracts::{GasToken, NativeContract};
use neo_primitives::{CallFlags, UInt160, UInt256, WitnessScope};
use neo_protocol::ProtocolSettings;
use neo_protocol::execution::Block as ExecutionBlock;
use neo_protocol::payloads::{Block, Signer, Transaction};
use neo_protocol::witness::Witness;
use neo_smart_contract::application_engine::TEST_MODE_GAS;
use neo_system::{ExecutionMode, NeoSystem};
use neo_vm::{OpCode, ScriptBuilder, VmState};

fn transfer_script(contract: UInt160, from: UInt160, to: UInt160, amount: i64) -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    builder.emit_opcode(OpCode::PUSHNULL);
    builder.emit_push_int(amount);
    builder.emit_push(&to.to_bytes());
    builder.emit_push(&from.to_bytes());
    builder.emit_push_int(4);
    builder.emit_opcode(OpCode::PACK);
    builder.emit_push_int(CallFlags::ALL.bits() as i64);
    builder.emit_push(b"transfer");
    builder.emit_push(&contract.to_bytes());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("syscall");
    builder.emit_opcode(OpCode::ASSERT);
    builder.to_array()
}

fn transfer_transaction(
    height: u32,
    contract: UInt160,
    from: UInt160,
    to: UInt160,
    amount: i64,
) -> Transaction {
    let mut transaction = Transaction::new();
    transaction.set_nonce(height);
    transaction.set_system_fee(TEST_MODE_GAS);
    transaction.set_network_fee(0);
    transaction.set_valid_until_block(height.saturating_add(1_000));
    transaction.set_signers(vec![Signer::new(from, WitnessScope::CALLED_BY_ENTRY)]);
    transaction.set_witnesses(vec![Witness::new()]);
    transaction.set_script(transfer_script(contract, from, to, amount));
    transaction
}

fn transfer_block(
    height: u32,
    previous_hash: UInt256,
    contract: UInt160,
    from: UInt160,
    to: UInt160,
) -> Block {
    let mut block = Block::new();
    block.header.set_index(height);
    block.header.set_prev_hash(previous_hash);
    block
        .header
        .set_timestamp(1_700_000_000_000 + u64::from(height) * 15_000);
    block.header.set_next_consensus(UInt160::zero());
    block.transactions = vec![transfer_transaction(height, contract, from, to, 1)];
    block.rebuild_merkle_root();
    block
}

fn bench_transfer_modes(c: &mut Criterion) {
    for mode in [
        ExecutionMode::Canonical,
        ExecutionMode::Auto,
        ExecutionMode::Verify,
    ] {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let _guard = runtime.enter();
        let settings = ProtocolSettings::testnet();
        let sender = NativeHelpers::get_bft_address(&settings.standby_validators());
        let recipient = UInt160::from_bytes(&[0x42; 20]).expect("recipient");
        let contract = GasToken::new().hash();
        let system = NeoSystem::new(settings, None, None).expect("neo system");
        let mut height = system.current_block_index();
        let mut previous_hash = system.genesis_block().hash();

        let benchmark_name = format!("persist_one_gas_transfer/{}", mode.as_str());
        c.bench_function(&benchmark_name, |b| {
            b.iter(|| {
                height = height.saturating_add(1);
                let mut block = transfer_block(height, previous_hash, contract, sender, recipient);
                previous_hash = block.hash();
                let executions = system
                    .persist_block_with_receipts_and_execution_mode(
                        ExecutionBlock::from(&block),
                        mode,
                    )
                    .expect("persist transfer");
                assert_eq!(executions[1].vm_state, VmState::HALT);
                black_box(executions);
            });
        });
    }
}

fn bench_ten_transfer_modes(c: &mut Criterion) {
    for mode in [
        ExecutionMode::Canonical,
        ExecutionMode::Auto,
        ExecutionMode::Verify,
    ] {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let _guard = runtime.enter();
        let settings = ProtocolSettings::testnet();
        let sender = NativeHelpers::get_bft_address(&settings.standby_validators());
        let recipient = UInt160::from_bytes(&[0x42; 20]).expect("recipient");
        let contract = GasToken::new().hash();
        let system = NeoSystem::new(settings, None, None).expect("neo system");
        let mut height = system.current_block_index();
        let mut previous_hash = system.genesis_block().hash();

        let benchmark_name = format!("persist_ten_gas_transfers/{}", mode.as_str());
        c.bench_function(&benchmark_name, |b| {
            b.iter(|| {
                height = height.saturating_add(1);
                let mut block = Block::new();
                block.header.set_index(height);
                block.header.set_prev_hash(previous_hash);
                block
                    .header
                    .set_timestamp(1_700_000_000_000 + u64::from(height) * 15_000);
                block.header.set_next_consensus(UInt160::zero());
                block.transactions = (0..10)
                    .map(|offset| {
                        transfer_transaction(
                            height.saturating_mul(16).saturating_add(offset),
                            contract,
                            sender,
                            recipient,
                            1,
                        )
                    })
                    .collect();
                block.rebuild_merkle_root();
                previous_hash = block.hash();
                let executions = system
                    .persist_block_with_receipts_and_execution_mode(
                        ExecutionBlock::from(&block),
                        mode,
                    )
                    .expect("persist transfers");
                assert_eq!(executions.len(), 12);
                assert!(
                    executions[1..=10]
                        .iter()
                        .all(|execution| execution.vm_state == VmState::HALT)
                );
                black_box(executions);
            });
        });
    }
}

criterion_group!(benches, bench_transfer_modes, bench_ten_transfer_modes);
criterion_main!(benches);
