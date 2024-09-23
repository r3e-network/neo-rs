use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::thread;
use fakechain::FakeChain;
use block::Block;
use zaptest::Logger;
use assert::assert_eq;
use assert::assert_no_error;

#[test]
fn test_block_queue() {
    let chain = Arc::new(Mutex::new(FakeChain::new()));
    // notice, it's not yet running
    let bq = Arc::new(Mutex::new(Queue::new(chain.clone(), Logger::new(), None, 0, None, NonBlocking)));
    let mut blocks = vec![None; 11];
    for i in 1..11 {
        blocks[i] = Some(Block { header: block::Header { index: i as u32 } });
    }
    // not the ones expected currently
    for i in 3..5 {
        assert_no_error!(bq.lock().unwrap().put_block(blocks[i].as_ref().unwrap()));
    }
    let (last, cap_left) = bq.lock().unwrap().last_queued();
    assert_eq!(0, last);
    assert_eq!(DefaultCacheSize - 2, cap_left);
    // nothing should be put into the blockchain
    assert_eq!(0, chain.lock().unwrap().block_height());
    assert_eq!(2, bq.lock().unwrap().length());
    // now added the expected ones (with duplicates)
    for i in 1..5 {
        assert_no_error!(bq.lock().unwrap().put_block(blocks[i].as_ref().unwrap()));
    }
    // but they're still not put into the blockchain, because bq isn't running
    let (last, cap_left) = bq.lock().unwrap().last_queued();
    assert_eq!(4, last);
    assert_eq!(DefaultCacheSize - 4, cap_left);
    assert_eq!(0, chain.lock().unwrap().block_height());
    assert_eq!(4, bq.lock().unwrap().length());
    // block with too big index is dropped
    assert_no_error!(bq.lock().unwrap().put_block(&Block { header: block::Header { index: chain.lock().unwrap().block_height() + DefaultCacheSize + 1 } }));
    assert_eq!(4, bq.lock().unwrap().length());
    let bq_clone = bq.clone();
    thread::spawn(move || {
        bq_clone.lock().unwrap().run();
    });
    // run() is asynchronous, so we need some kind of timeout anyway and this is the simplest one
    assert_eventually!(|| chain.lock().unwrap().block_height() == 4, Duration::from_secs(4), Duration::from_millis(100));
    let (last, cap_left) = bq.lock().unwrap().last_queued();
    assert_eq!(4, last);
    assert_eq!(DefaultCacheSize, cap_left);
    assert_eq!(0, bq.lock().unwrap().length());
    assert_eq!(4, chain.lock().unwrap().block_height());
    // put some old blocks
    for i in 1..5 {
        assert_no_error!(bq.lock().unwrap().put_block(blocks[i].as_ref().unwrap()));
    }
    let (last, cap_left) = bq.lock().unwrap().last_queued();
    assert_eq!(4, last);
    assert_eq!(DefaultCacheSize, cap_left);
    assert_eq!(0, bq.lock().unwrap().length());
    assert_eq!(4, chain.lock().unwrap().block_height());
    // unexpected blocks with run() active
    assert_no_error!(bq.lock().unwrap().put_block(blocks[8].as_ref().unwrap()));
    assert_eq!(1, bq.lock().unwrap().length());
    assert_eq!(4, chain.lock().unwrap().block_height());
    assert_no_error!(bq.lock().unwrap().put_block(blocks[7].as_ref().unwrap()));
    assert_eq!(2, bq.lock().unwrap().length());
    assert_eq!(4, chain.lock().unwrap().block_height());
    // sparse put
    assert_no_error!(bq.lock().unwrap().put_block(blocks[10].as_ref().unwrap()));
    assert_eq!(3, bq.lock().unwrap().length());
    assert_eq!(4, chain.lock().unwrap().block_height());
    assert_no_error!(bq.lock().unwrap().put_block(blocks[6].as_ref().unwrap()));
    assert_no_error!(bq.lock().unwrap().put_block(blocks[5].as_ref().unwrap()));
    // run() is asynchronous, so we need some kind of timeout anyway and this is the simplest one
    assert_eventually!(|| chain.lock().unwrap().block_height() == 8, Duration::from_secs(4), Duration::from_millis(100));
    let (last, cap_left) = bq.lock().unwrap().last_queued();
    assert_eq!(8, last);
    assert_eq!(DefaultCacheSize - 1, cap_left);
    assert_eq!(1, bq.lock().unwrap().length());
    assert_eq!(8, chain.lock().unwrap().block_height());
    bq.lock().unwrap().discard();
    assert_eq!(0, bq.lock().unwrap().length());
}

// length wraps len access for tests to make them thread-safe.
impl Queue {
    fn length(&self) -> usize {
        let _lock = self.queue_lock.lock().unwrap();
        self.len
    }
}
