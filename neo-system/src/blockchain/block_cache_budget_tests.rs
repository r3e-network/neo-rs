//
// block_cache_budget_tests.rs - Shared block cache budget in the Blockchain actor
//

use super::*;
use crate::NeoSystem;
use crate::network::p2p::payloads::{block::Block as PayloadBlock, transaction::Transaction};
use neo_protocol::witness::Witness;
use neo_vm::OpCode;

/// Script length that makes a [`sized_block`] several times the cost of an
/// empty [`chain`] block.
const FAR_SCRIPT_LEN: usize = 2_000;

fn sized_payload(index: u32, script_len: usize) -> PayloadBlock {
    let mut tx = Transaction::new();
    tx.set_script(vec![OpCode::NOP.byte(); script_len]);
    let mut payload = PayloadBlock::new();
    payload.header.set_index(index);
    payload.header.witness = Witness::new();
    payload.transactions = vec![tx];
    payload.rebuild_merkle_root();
    payload
}

/// A block at `index` that passes the body integrity check and whose cost
/// grows with `script_len`. Its header is not part of any chain.
fn sized_block(index: u32, script_len: usize) -> Arc<Block> {
    Arc::new(Block::from(&sized_payload(index, script_len)))
}

fn far_block(index: u32) -> Arc<Block> {
    sized_block(index, FAR_SCRIPT_LEN)
}

/// Empty blocks `1..=length`, each naming its predecessor, that a node with
/// default settings persists.
fn chain(length: u32) -> Vec<Arc<Block>> {
    let mut previous_hash = UInt256::zero();
    (1..=length)
        .map(|index| {
            let mut block = Block::default();
            block.header.index = index;
            block.header.previous_hash = previous_hash;
            block.header.merkle_root = UInt256::zero();
            block.header.next_consensus = UInt160::zero();
            block.header.timestamp = u64::from(index);
            block.header.witnesses = vec![Witness::new()];
            previous_hash = block.hash();
            Arc::new(block)
        })
        .collect()
}

/// Makes the headers of `chain` known, as header sync does, so that its
/// blocks are verified against them when they arrive.
fn announce_headers(context: &NeoSystemContext, chain: &[Arc<Block>]) {
    let header_cache = context.header_cache();
    for block in chain {
        assert!(header_cache.add(block.to_payload_block().header));
    }
}

fn cost(block: &Block) -> usize {
    execution_block_cache_cost(block)
}

/// Holds all but `headroom` bytes of the budget `ledger` charges, the way
/// blocks cached elsewhere would, after emptying the body cache of `ledger`.
fn leave_headroom(ledger: &LedgerContext, headroom: usize) -> BlockCacheReservation {
    ledger.evict_block_bodies(usize::MAX);
    let budget = ledger.block_cache_budget();
    budget
        .try_reserve(budget.available() - headroom)
        .expect("ballast fits the budget")
}

/// An actor over a ledger context of its own whose budget is `limit` bytes.
fn actor_with_budget(limit: usize) -> Blockchain {
    Blockchain::new(Arc::new(LedgerContext::with_capacities_and_budget(
        64,
        8,
        BlockCacheBudget::new(limit),
    )))
}

/// Budget bytes held by the two actor caches. A block both of them hold
/// shares one reservation and is counted once.
fn actor_retained_bytes(blockchain: &Blockchain) -> usize {
    let mut seen = HashSet::new();
    let mut total = 0usize;
    let mut count = |entry: &RetainedBlock| {
        if seen.insert(Arc::as_ptr(&entry._reservation)) {
            assert_eq!(entry._reservation.bytes(), entry.cost);
            total += entry.cost;
        }
    };
    for entry in blockchain._block_cache.iter() {
        count(entry.value());
    }
    for entry in blockchain._block_cache_unverified.iter() {
        entry.value().iter().for_each(&mut count);
    }
    total
}

fn unverified_heights(blockchain: &Blockchain) -> Vec<u32> {
    let mut heights: Vec<u32> = blockchain
        ._block_cache_unverified
        .iter()
        .map(|entry| *entry.key())
        .collect();
    heights.sort_unstable();
    heights
}

/// Checks that the budget is within its limit and that every retained byte
/// is accounted for by a cache. `foreign` is what the test itself holds.
fn audit(blockchain: &Blockchain, foreign: usize) {
    let budget = &blockchain._block_cache_budget;
    let retained = budget.retained();
    assert!(
        retained <= budget.limit(),
        "retained {retained} bytes with a limit of {}",
        budget.limit()
    );
    assert_eq!(
        retained,
        foreign + blockchain.ledger.cached_block_bytes() + actor_retained_bytes(blockchain),
        "every retained byte belongs to a cached block"
    );

    let (bytes, blocks) = blockchain._block_cache_unverified.iter().fold(
        (0usize, 0usize),
        |(bytes, blocks), entry| {
            let list = entry.value();
            (
                bytes + list.iter().map(|block| block.cost).sum::<usize>(),
                blocks + list.len(),
            )
        },
    );
    assert_eq!(
        blockchain._unverified_cache_bytes.load(Ordering::Relaxed),
        bytes
    );
    assert_eq!(
        blockchain._unverified_cache_blocks.load(Ordering::Relaxed),
        blocks
    );
}

/// Empties every block cache through the paths that normally remove entries.
fn clear_block_caches(blockchain: &Blockchain) {
    for height in unverified_heights(blockchain) {
        blockchain.remove_unverified_index(height);
    }
    blockchain._block_cache.clear();
    blockchain.ledger.evict_block_bodies(usize::MAX);
}

#[tokio::test(flavor = "multi_thread")]
async fn block_caches_stay_within_the_shared_budget_through_a_sync() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    let mut blockchain = Blockchain::new(system.ledger_context());
    blockchain.system_context = Some(system.context());
    let ledger = system.ledger_context();
    let budget = ledger.block_cache_budget();

    let chain = chain(8);
    announce_headers(&system.context(), &chain);
    let ballast = leave_headroom(
        &ledger,
        2 * cost(&far_block(50)) + 3 * cost(chain[0].as_ref()),
    );
    audit(&blockchain, ballast.bytes());

    // Blocks nobody can verify yet alternate with verified blocks arriving
    // from the highest height down, so all three caches fill up and compete.
    for (step, block) in chain.iter().skip(1).rev().enumerate() {
        let far = far_block(50 + u32::try_from(step).expect("step fits u32"));
        assert_eq!(
            blockchain.on_new_block(far, true).await,
            VerifyResult::UnableToVerify
        );
        audit(&blockchain, ballast.bytes());

        assert_eq!(
            blockchain.on_new_block(Arc::clone(block), true).await,
            VerifyResult::Succeed,
            "verified block {} must not be kept out by far-future blocks",
            block.index()
        );
        audit(&blockchain, ballast.bytes());
    }
    for block in &chain[1..] {
        assert!(blockchain._block_cache.contains_key(&block.hash()));
    }

    assert_eq!(
        blockchain.on_new_block(Arc::clone(&chain[0]), true).await,
        VerifyResult::Succeed
    );
    assert_eq!(ledger.current_height(), 8);
    audit(&blockchain, ballast.bytes());
    assert!(
        ledger.cached_block_bytes() > 0,
        "persisted bodies are cached once pending blocks made room"
    );

    drop(ballast);
    clear_block_caches(&blockchain);
    audit(&blockchain, 0);
    assert_eq!(budget.retained(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn block_the_chain_needs_sooner_displaces_far_future_blocks() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    let mut blockchain = Blockchain::new(system.ledger_context());
    blockchain.system_context = Some(system.context());
    let ledger = system.ledger_context();
    let budget = ledger.block_cache_budget();

    let chain = chain(2);
    announce_headers(&system.context(), &chain);
    let chain_cost = cost(chain[0].as_ref());
    let ballast = leave_headroom(&ledger, 4 * cost(&far_block(100)) + chain_cost / 2);

    // A peer fills the budget with blocks far above any verified header.
    for height in 100..104 {
        assert_eq!(
            blockchain.on_new_block(far_block(height), true).await,
            VerifyResult::UnableToVerify
        );
    }
    assert_eq!(unverified_heights(&blockchain), [100, 101, 102, 103]);
    assert!(budget.available() < chain_cost);

    // Block 2 is verified but cannot be persisted before block 1. It is
    // admitted at the expense of the highest far-future block only.
    let retained_before = budget.retained();
    let block_two = Arc::clone(&chain[1]);
    assert_eq!(
        blockchain.on_new_block(Arc::clone(&block_two), true).await,
        VerifyResult::Succeed
    );
    assert!(blockchain._block_cache.contains_key(&block_two.hash()));
    assert_eq!(unverified_heights(&blockchain), [2, 100, 101, 102]);
    // Held by both actor caches, charged once.
    assert_eq!(
        budget.retained(),
        retained_before - cost(&far_block(103)) + chain_cost
    );
    audit(&blockchain, ballast.bytes());

    assert_eq!(
        blockchain.on_new_block(Arc::clone(&chain[0]), true).await,
        VerifyResult::Succeed
    );
    assert_eq!(ledger.current_height(), 2);
    assert_eq!(unverified_heights(&blockchain), [100, 101, 102]);
    audit(&blockchain, ballast.bytes());
}

#[tokio::test(flavor = "multi_thread")]
async fn next_block_is_persisted_when_the_budget_has_no_room_for_it() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    let mut blockchain = Blockchain::new(system.ledger_context());
    blockchain.system_context = Some(system.context());
    let ledger = system.ledger_context();
    let budget = ledger.block_cache_budget();

    let chain = chain(1);
    announce_headers(&system.context(), &chain);
    let chain_cost = cost(chain[0].as_ref());
    let ballast = leave_headroom(&ledger, 2 * cost(&far_block(100)) + chain_cost / 2);
    for height in [100, 101] {
        assert_eq!(
            blockchain.on_new_block(far_block(height), true).await,
            VerifyResult::UnableToVerify
        );
    }
    assert!(budget.available() < chain_cost);
    let retained_before = budget.retained();

    let next = Arc::clone(&chain[0]);
    assert_eq!(
        blockchain.on_new_block(Arc::clone(&next), true).await,
        VerifyResult::Succeed
    );
    assert_eq!(ledger.current_height(), 1);

    // Persisted without being retained anywhere, and without anything having
    // been given up for it.
    assert!(!blockchain._block_cache.contains_key(&next.hash()));
    assert_eq!(ledger.cached_block_bytes(), 0);
    assert_eq!(unverified_heights(&blockchain), [100, 101]);
    assert_eq!(budget.retained(), retained_before);
    audit(&blockchain, ballast.bytes());

    // The body the cache could not hold is still served from the store.
    assert!(ledger.get_block(&next.hash()).is_none());
    assert!(system.context().try_get_block(&next.hash()).is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn verified_block_without_room_is_out_of_memory_until_there_is_room() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    let mut blockchain = Blockchain::new(system.ledger_context());
    blockchain.system_context = Some(system.context());
    let ledger = system.ledger_context();
    let budget = ledger.block_cache_budget();

    let chain = chain(2);
    announce_headers(&system.context(), &chain);
    let block_two = Arc::clone(&chain[1]);
    let ballast = leave_headroom(&ledger, cost(block_two.as_ref()) - 1);
    let retained_before = budget.retained();

    // Nothing of lower priority is cached, so the block cannot be admitted.
    // The verdict is the same however often it is offered.
    for _ in 0..2 {
        assert_eq!(
            blockchain.on_new_block(Arc::clone(&block_two), true).await,
            VerifyResult::OutOfMemory
        );
        assert!(blockchain._block_cache.is_empty());
        assert!(blockchain._block_cache_unverified.is_empty());
        assert_eq!(budget.retained(), retained_before);
    }

    drop(ballast);
    assert_eq!(
        blockchain.on_new_block(block_two, true).await,
        VerifyResult::Succeed
    );
    assert_eq!(
        blockchain.on_new_block(Arc::clone(&chain[0]), true).await,
        VerifyResult::Succeed
    );
    assert_eq!(ledger.current_height(), 2);
    audit(&blockchain, 0);
}

/// Offers blocks 3, 2, 1 of `chain` and then block 3 again, and returns the
/// verdicts together with the durable tip.
async fn sync_three_blocks_out_of_order(
    chain: &[Arc<Block>],
    headroom: Option<usize>,
) -> (Vec<VerifyResult>, u32, UInt256) {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    let mut blockchain = Blockchain::new(system.ledger_context());
    blockchain.system_context = Some(system.context());
    let ledger = system.ledger_context();
    announce_headers(&system.context(), chain);
    let ballast = headroom.map(|headroom| leave_headroom(&ledger, headroom));
    let foreign = ballast.as_ref().map_or(0, BlockCacheReservation::bytes);

    let mut verdicts = Vec::new();
    for index in [2usize, 1, 0, 2] {
        if headroom.is_some() && index == 0 {
            // Block 2 took the only room there is, so block 3 was given up.
            assert!(!blockchain._block_cache.contains_key(&chain[2].hash()));
            assert_eq!(unverified_heights(&blockchain), [2]);
        }
        verdicts.push(
            blockchain
                .on_new_block(Arc::clone(&chain[index]), true)
                .await,
        );
        if headroom.is_some() {
            audit(&blockchain, foreign);
        }
    }

    let store_cache = system.context().store_cache();
    let ledger_contract = LedgerContract::new();
    (
        verdicts,
        ledger_contract
            .current_index(&store_cache)
            .expect("current index"),
        ledger_contract
            .current_hash(&store_cache)
            .expect("current hash"),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn evicted_verified_block_is_accepted_again_and_persisted_the_same() {
    let chain = chain(3);
    let chain_cost = cost(chain[0].as_ref());

    // With room for a single block, block 3 is evicted when block 2 arrives
    // and has to come back after the chain reached height 2.
    let (verdicts, index, hash) =
        sync_three_blocks_out_of_order(&chain, Some(chain_cost + chain_cost / 2)).await;
    assert_eq!(verdicts, [VerifyResult::Succeed; 4]);

    // With the default budget nothing is evicted and the repeated block 3 is
    // already persisted.
    let (control_verdicts, control_index, control_hash) =
        sync_three_blocks_out_of_order(&chain, None).await;
    assert_eq!(
        control_verdicts,
        [
            VerifyResult::Succeed,
            VerifyResult::Succeed,
            VerifyResult::Succeed,
            VerifyResult::AlreadyExists
        ]
    );

    assert_eq!((index, hash), (3, chain[2].hash()));
    assert_eq!((index, hash), (control_index, control_hash));
}

#[tokio::test(flavor = "multi_thread")]
async fn evicted_unverified_block_gets_the_same_verdict_when_it_comes_back() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    let mut blockchain = Blockchain::new(system.ledger_context());
    blockchain.system_context = Some(system.context());
    let ledger = system.ledger_context();
    let ballast = leave_headroom(&ledger, 2 * cost(&far_block(60)) + 1);

    let evicted = far_block(70);
    for block in [far_block(60), Arc::clone(&evicted), far_block(50)] {
        assert_eq!(
            blockchain.on_new_block(block, true).await,
            VerifyResult::UnableToVerify
        );
    }
    // The lowest newcomer took the place of the highest block.
    assert_eq!(unverified_heights(&blockchain), [50, 60]);

    // Offered again while there is no room: same verdict, still not cached.
    assert_eq!(
        blockchain.on_new_block(Arc::clone(&evicted), true).await,
        VerifyResult::UnableToVerify
    );
    assert_eq!(unverified_heights(&blockchain), [50, 60]);
    audit(&blockchain, ballast.bytes());

    // Offered again once there is room: same verdict, cached.
    drop(ballast);
    assert_eq!(
        blockchain.on_new_block(Arc::clone(&evicted), true).await,
        VerifyResult::UnableToVerify
    );
    assert_eq!(unverified_heights(&blockchain), [50, 60, 70]);
    assert!(
        blockchain
            ._block_cache_unverified
            .get(&70)
            .expect("height cached")
            .iter()
            .any(|candidate| candidate.hash == evicted.hash())
    );
    audit(&blockchain, 0);
}

#[tokio::test]
async fn higher_blocks_never_displace_lower_ones() {
    let block_cost = cost(&far_block(10));
    let blockchain = actor_with_budget(3 * block_cost + block_cost / 2);

    for height in [10, 20, 30] {
        blockchain.add_unverified_block(far_block(height)).await;
    }
    assert_eq!(unverified_heights(&blockchain), [10, 20, 30]);

    // Above everything cached: refused, nothing is given up.
    blockchain.add_unverified_block(far_block(40)).await;
    assert_eq!(unverified_heights(&blockchain), [10, 20, 30]);

    // Below something cached: the highest block gives way, and only it.
    blockchain.add_unverified_block(far_block(15)).await;
    assert_eq!(unverified_heights(&blockchain), [10, 15, 20]);
    blockchain.add_unverified_block(far_block(5)).await;
    assert_eq!(unverified_heights(&blockchain), [5, 10, 15]);

    // A second candidate at a cached height does not displace that height.
    blockchain
        .add_unverified_block(sized_block(15, FAR_SCRIPT_LEN + 1))
        .await;
    assert_eq!(unverified_heights(&blockchain), [5, 10, 15]);
    assert_eq!(
        blockchain
            ._block_cache_unverified
            .get(&15)
            .expect("height cached")
            .len(),
        1
    );
    audit(&blockchain, 0);
}

#[tokio::test]
async fn full_unverified_cache_gives_up_higher_blocks_only() {
    let blockchain = actor_with_budget(neo_ledger::DEFAULT_BLOCK_CACHE_BUDGET_BYTES);
    let empty_block = |index: u32| {
        let mut block = Block::default();
        block.header.index = index;
        block.header.witnesses = vec![Witness::new()];
        Arc::new(block)
    };
    let capacity = u32::try_from(MAX_UNVERIFIED_CACHE_SIZE).expect("capacity fits u32");
    let lowest = 1_000;
    let highest = lowest + capacity - 1;

    for index in lowest..=highest {
        blockchain.add_unverified_block(empty_block(index)).await;
    }
    assert_eq!(
        blockchain._unverified_cache_blocks.load(Ordering::Relaxed),
        MAX_UNVERIFIED_CACHE_SIZE
    );

    // Above every cached block: refused, nothing is given up.
    blockchain
        .add_unverified_block(empty_block(highest + 1))
        .await;
    assert!(
        !blockchain
            ._block_cache_unverified
            .contains_key(&(highest + 1))
    );
    assert!(blockchain._block_cache_unverified.contains_key(&highest));

    // Below a cached block: the highest one gives way.
    blockchain
        .add_unverified_block(empty_block(lowest - 1))
        .await;
    assert!(
        blockchain
            ._block_cache_unverified
            .contains_key(&(lowest - 1))
    );
    assert!(!blockchain._block_cache_unverified.contains_key(&highest));
    assert!(
        blockchain
            ._block_cache_unverified
            .contains_key(&(highest - 1))
    );
    assert_eq!(
        blockchain._unverified_cache_blocks.load(Ordering::Relaxed),
        MAX_UNVERIFIED_CACHE_SIZE
    );
    audit(&blockchain, 0);
}

#[tokio::test]
async fn stale_blocks_and_ledger_bodies_give_way_before_pending_blocks() {
    let block_cost = cost(&far_block(10));
    let blockchain = actor_with_budget(4 * block_cost + block_cost / 2);
    let ledger = Arc::clone(&blockchain.ledger);

    // Two persisted bodies in the ledger context, heights 1 and 2.
    let first_body = ledger
        .insert_block(sized_payload(1, FAR_SCRIPT_LEN))
        .expect("first body");
    let second_body = ledger
        .insert_block(sized_payload(2, FAR_SCRIPT_LEN))
        .expect("second body");
    for height in [10, 20] {
        blockchain.add_unverified_block(far_block(height)).await;
    }
    audit(&blockchain, 0);
    assert_eq!(blockchain._block_cache_budget.available(), block_cost / 2);

    // A block parked at a height that has been persisted since is of no use
    // and goes first, before any body.
    ledger.record_tip(10);
    blockchain.add_unverified_block(far_block(30)).await;
    assert_eq!(unverified_heights(&blockchain), [20, 30]);
    assert_eq!(ledger.cached_block_count(), 2);

    // Then the persisted bodies, oldest first, even for a block above every
    // pending one.
    blockchain.add_unverified_block(far_block(40)).await;
    assert_eq!(unverified_heights(&blockchain), [20, 30, 40]);
    assert!(ledger.get_block(&first_body).is_none());
    assert!(ledger.get_block(&second_body).is_some());

    blockchain.add_unverified_block(far_block(50)).await;
    assert_eq!(unverified_heights(&blockchain), [20, 30, 40, 50]);
    assert_eq!(ledger.cached_block_count(), 0);

    // Only now is there nothing left to give up for a higher block.
    blockchain.add_unverified_block(far_block(60)).await;
    assert_eq!(unverified_heights(&blockchain), [20, 30, 40, 50]);
    audit(&blockchain, 0);
}

#[tokio::test]
async fn block_larger_than_the_budget_is_never_retained_and_evicts_nothing() {
    let block_cost = cost(&far_block(10));
    let limit = 2 * block_cost + block_cost / 2;
    let blockchain = actor_with_budget(limit);
    let ledger = Arc::clone(&blockchain.ledger);
    let body = ledger
        .insert_block(sized_payload(1, 10))
        .expect("persisted body");
    for height in [10, 20] {
        blockchain.add_unverified_block(far_block(height)).await;
    }
    let retained_before = blockchain._block_cache_budget.retained();

    // Lower than everything pending, so it would outrank all of it, but no
    // amount of eviction can make it fit.
    let oversize = sized_block(5, 3 * FAR_SCRIPT_LEN);
    assert!(cost(&oversize) > limit);

    blockchain.add_unverified_block(Arc::clone(&oversize)).await;
    assert_eq!(unverified_heights(&blockchain), [10, 20]);

    // The admission step of a verified block refuses it the same way.
    assert!(
        blockchain
            .retain_block(Arc::clone(&oversize), oversize.hash())
            .is_none()
    );
    assert!(
        blockchain
            .try_retain_block(Arc::clone(&oversize), oversize.hash())
            .is_none()
    );

    assert_eq!(unverified_heights(&blockchain), [10, 20]);
    assert!(ledger.get_block(&body).is_some());
    assert_eq!(blockchain._block_cache_budget.retained(), retained_before);
    audit(&blockchain, 0);
}

#[tokio::test]
async fn every_way_out_of_the_actor_caches_returns_the_bytes() {
    let block_cost = cost(&far_block(10));
    let budget = BlockCacheBudget::new(16 * block_cost);
    let ledger = Arc::new(LedgerContext::with_capacities_and_budget(
        64,
        8,
        Arc::clone(&budget),
    ));
    let blockchain = Blockchain::new(Arc::clone(&ledger));

    // Three candidates at one height, then a fourth that evicts the oldest.
    let candidates: Vec<Arc<Block>> = (0..4)
        .map(|extra| sized_block(10, FAR_SCRIPT_LEN + extra))
        .collect();
    for candidate in &candidates {
        blockchain.add_unverified_block(Arc::clone(candidate)).await;
        audit(&blockchain, 0);
    }
    assert_eq!(
        blockchain._unverified_cache_blocks.load(Ordering::Relaxed),
        MAX_UNVERIFIED_CANDIDATES_PER_HEIGHT
    );
    assert_eq!(
        budget.retained(),
        candidates[1..]
            .iter()
            .map(|block| cost(block))
            .sum::<usize>()
    );

    // A duplicate is not charged a second time.
    blockchain
        .add_unverified_block(Arc::clone(&candidates[3]))
        .await;
    assert_eq!(
        blockchain._unverified_cache_blocks.load(Ordering::Relaxed),
        MAX_UNVERIFIED_CANDIDATES_PER_HEIGHT
    );
    assert_eq!(
        budget.retained(),
        candidates[1..]
            .iter()
            .map(|block| cost(block))
            .sum::<usize>()
    );
    audit(&blockchain, 0);

    // Removal of a whole height.
    blockchain.add_unverified_block(far_block(20)).await;
    assert!(blockchain.remove_unverified_index(10).is_some());
    audit(&blockchain, 0);
    assert_eq!(budget.retained(), block_cost);

    // A verified cache entry handed over to its taker, then dropped.
    let verified = far_block(30);
    let entry = blockchain
        .retain_block(Arc::clone(&verified), verified.hash())
        .expect("verified block fits");
    blockchain._block_cache.insert(entry.hash, entry);
    audit(&blockchain, 0);
    assert!(blockchain.find_block_in_cache_by_index(31).is_none());
    let taken = blockchain
        .find_block_in_cache_by_index(30)
        .expect("block cached at height 30");
    assert!(blockchain._block_cache.is_empty());
    assert_eq!(budget.retained(), 2 * block_cost);
    drop(taken);
    assert_eq!(budget.retained(), block_cost);

    // Dropping the actor returns what it still held; dropping the ledger
    // context returns the bodies.
    ledger
        .insert_block(sized_payload(1, FAR_SCRIPT_LEN))
        .expect("persisted body");
    assert_eq!(budget.retained(), 2 * block_cost);
    drop(blockchain);
    assert_eq!(budget.retained(), ledger.cached_block_bytes());
    assert_eq!(budget.retained(), block_cost);
    drop(ledger);
    assert_eq!(budget.retained(), 0);
}
