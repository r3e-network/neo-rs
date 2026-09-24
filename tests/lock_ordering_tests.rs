//! Lock ordering validation tests
//!
//! These tests verify that locks are acquired in the correct order
//! as documented in `docs/LOCK_ORDERING.md`.

use neo_core::ledger::LedgerContext;
use neo_core::network::p2p::payloads::block::Block;
use neo_core::network::p2p::payloads::header::Header;
use neo_core::UInt256;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Test that LedgerContext can handle concurrent access without deadlocking
#[test]
fn ledger_context_concurrent_access() {
    let context = Arc::new(LedgerContext::default());
    let mut handles = vec![];

    // Spawn multiple threads that perform concurrent operations
    for i in 0..10 {
        let ctx = Arc::clone(&context);
        let handle = thread::spawn(move || {
            for j in 0..100 {
                // Mix of read and write operations
                if j % 3 == 0 {
                    let _ = ctx.current_height();
                    let _ = ctx.highest_header_index();
                } else if j % 3 == 1 {
                    let _ = ctx.block_hash_at(i);
                    let _ = ctx.headers_from_index(i, 5);
                } else {
                    let _ = ctx.mempool_transaction_hashes();
                }
                thread::sleep(Duration::from_micros(10));
            }
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().expect("Thread panicked");
    }
}

/// Test that insert_block follows correct lock ordering
#[test]
fn ledger_context_insert_block_ordering() {
    let context = Arc::new(LedgerContext::default());
    let mut handles = vec![];

    // Spawn threads that insert blocks concurrently
    for i in 0..5 {
        let ctx = Arc::clone(&context);
        let handle = thread::spawn(move || {
            for j in 0..20 {
                let mut block = Block::new();
                let mut header = Header::new();
                header.set_index(i * 100 + j);
                block.header = header;

                // This should not deadlock as it follows correct lock order
                let result = ctx.insert_block(block);
                assert!(result.is_ok());

                thread::sleep(Duration::from_micros(50));
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Thread panicked during insert_block");
    }
}

/// Test concurrent reads and writes to different lock levels
#[test]
fn ledger_context_mixed_operations() {
    let context = Arc::new(LedgerContext::default());
    let mut handles = vec![];

    // Reader threads
    for _ in 0..5 {
        let ctx = Arc::clone(&context);
        let handle = thread::spawn(move || {
            for i in 0..100 {
                let _ = ctx.block_hash_at(i);
                let _ = ctx.get_transaction(&UInt256::zero());
                thread::sleep(Duration::from_micros(10));
            }
        });
        handles.push(handle);
    }

    // Writer threads
    for i in 0..3 {
        let ctx = Arc::clone(&context);
        let handle = thread::spawn(move || {
            for j in 0..50 {
                let mut block = Block::new();
                let mut header = Header::new();
                header.set_index(i * 1000 + j);
                block.header = header;

                let _ = ctx.insert_block(block);
                thread::sleep(Duration::from_micros(20));
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Thread panicked during mixed operations");
    }
}

/// Test that record_tip is lock-free and doesn't cause contention
#[test]
fn ledger_context_record_tip_concurrent() {
    let context = Arc::new(LedgerContext::default());
    let mut handles = vec![];

    for i in 0..20 {
        let ctx = Arc::clone(&context);
        let handle = thread::spawn(move || {
            for j in 0..1000 {
                ctx.record_tip(i * 1000 + j);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Thread panicked during record_tip");
    }

    // Verify atomic operations worked correctly
    assert!(context.current_height() >= 19000);
}

