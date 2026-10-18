//! P2P frame encode/decode benchmarks for block-sized payloads.
//!
//! This measures `NetworkMessage::to_bytes` (including optional LZ4
//! compression) and `NetworkMessage::from_bytes` for a 500-transaction block.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use neo_core::WitnessScope;
use neo_core::neo_io::{MemoryReader, Serializable};
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::network::p2p::payloads::{Block, Transaction};
use neo_core::network::p2p::{Message, NetworkMessage, ProtocolMessage};
use neo_core::UInt160;

fn block_message(transaction_count: usize) -> NetworkMessage {
    let mut block = Block::new();
    let mut transactions: Vec<Transaction> = (0..transaction_count)
        .map(|_| {
            let mut tx = Transaction::new();
            tx.add_signer(Signer::new(UInt160::zero(), WitnessScope::NONE));
            tx.add_witness(Witness::new());
            tx.set_script(vec![0x40]); // RET
            tx
        })
        .collect();
    let hashes: Vec<_> = transactions.iter_mut().map(|tx| tx.hash()).collect();
    if let Some(root) = neo_core::cryptography::MerkleTree::compute_root(&hashes) {
        block.header.set_merkle_root(root);
    }
    block.transactions = transactions;
    NetworkMessage::new(ProtocolMessage::Block(block))
}

fn bench_p2p_block_frames(c: &mut Criterion) {
    let message = block_message(500);
    let wire = message
        .to_bytes(true)
        .expect("serialize compressed block message");

    c.bench_function("p2p_block_encode_500tx_compressed", |b| {
        b.iter(|| black_box(message.to_bytes(true).expect("encode")));
    });

    c.bench_function("p2p_block_decode_500tx_compressed", |b| {
        b.iter(|| black_box(NetworkMessage::from_bytes(black_box(&wire)).expect("decode")));
    });

    let mut reader = MemoryReader::new(&wire);
    let message = Message::deserialize(&mut reader).expect("message deserialize");

    c.bench_function("p2p_message_deserialize_500tx_compressed", |b| {
        b.iter(|| {
            let mut reader = MemoryReader::new(black_box(&wire));
            black_box(Message::deserialize(&mut reader).expect("message"));
        });
    });

    c.bench_function("p2p_protocol_decode_500tx", |b| {
        b.iter(|| black_box(message.to_protocol_message().expect("protocol")));
    });
}

criterion_group!(benches, bench_p2p_block_frames);
criterion_main!(benches);
