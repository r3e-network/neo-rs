use std::sync::{Arc, RwLock, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::thread;
use std::sync::mpsc::{self, Sender, Receiver};

use crate::core::block;
use log::warn;

// Blockqueuer is an interface for a block queue.
pub trait Blockqueuer {
    fn add_block(&self, block: Arc<block::Block>) -> Result<(), String>;
    fn add_headers(&self, headers: Vec<Arc<block::Header>>) -> Result<(), String>;
    fn block_height(&self) -> u32;
}

// OperationMode is the mode of operation for the block queue.
// Could be either Blocking or NonBlocking.
#[derive(Clone, Copy)]
pub enum OperationMode {
    NonBlocking = 0,
    Blocking = 1,
}

// Queue is the block queue.
pub struct Queue {
    log: Arc<Mutex<log::Logger>>,
    queue_lock: RwLock<()>,
    queue: Vec<Option<Arc<block::Block>>>,
    last_q: u32,
    check_blocks: Sender<()>,
    chain: Arc<dyn Blockqueuer + Send + Sync>,
    relay_f: Option<Arc<dyn Fn(Arc<block::Block>) + Send + Sync>>,
    discarded: AtomicBool,
    len: Mutex<usize>,
    len_update_f: Option<Arc<dyn Fn(usize) + Send + Sync>>,
    cache_size: usize,
    mode: OperationMode,
}

// DefaultCacheSize is the default amount of blocks above the current height
// which are stored in the queue.
pub const DEFAULT_CACHE_SIZE: usize = 2000;

impl Queue {
    fn index_to_position(&self, i: u32) -> usize {
        (i as usize) % self.cache_size
    }

    // New creates an instance of BlockQueue.
    pub fn new(
        bc: Arc<dyn Blockqueuer + Send + Sync>,
        log: Arc<Mutex<log::Logger>>,
        relayer: Option<Arc<dyn Fn(Arc<block::Block>) + Send + Sync>>,
        cache_size: usize,
        len_metrics_updater: Option<Arc<dyn Fn(usize) + Send + Sync>>,
        mode: OperationMode,
    ) -> Arc<Self> {
        let cache_size = if cache_size <= 0 { DEFAULT_CACHE_SIZE } else { cache_size };
        let (tx, rx) = mpsc::channel();

        Arc::new(Queue {
            log,
            queue_lock: RwLock::new(()),
            queue: vec![None; cache_size],
            last_q: 0,
            check_blocks: tx,
            chain: bc,
            relay_f: relayer,
            discarded: AtomicBool::new(false),
            len: Mutex::new(0),
            len_update_f: len_metrics_updater,
            cache_size,
            mode,
        })
    }

    // Run runs the BlockQueue queueing loop. It must be called in a separate routine.
    pub fn run(self: Arc<Self>) {
        let mut last_height = self.chain.block_height();
        let rx = self.check_blocks.clone();
        thread::spawn(move || {
            loop {
                if rx.recv().is_err() {
                    break;
                }
                loop {
                    let h = self.chain.block_height();
                    let pos = self.index_to_position(h + 1);
                    let mut queue_lock = self.queue_lock.write().unwrap();
                    let b = self.queue[pos].clone();
                    // The chain moved forward using blocks from other sources (consensus).
                    for i in last_height..h {
                        let old = self.index_to_position(i + 1);
                        if let Some(ref block) = self.queue[old] {
                            if block.index == i {
                                *self.len.lock().unwrap() -= 1;
                                self.queue[old] = None;
                            }
                        }
                    }
                    drop(queue_lock);
                    last_height = h;
                    if b.is_none() {
                        break;
                    }

                    let b = b.unwrap();
                    if let Err(err) = self.chain.add_block(b.clone()) {
                        // The block might already be added by the consensus.
                        if self.chain.block_height() < b.index {
                            warn!(
                                "blockQueue: failed adding block into the blockchain: {}",
                                err
                            );
                        }
                    } else if let Some(ref relay_f) = self.relay_f {
                        relay_f(b.clone());
                    }
                    let mut queue_lock = self.queue_lock.write().unwrap();
                    *self.len.lock().unwrap() -= 1;
                    let l = *self.len.lock().unwrap();
                    if self.queue[pos] == Some(b.clone()) {
                        self.queue[pos] = None;
                    }
                    drop(queue_lock);
                    if let Some(ref len_update_f) = self.len_update_f {
                        len_update_f(l);
                    }
                }
            }
        });
    }

    // PutBlock enqueues block to be added to the chain.
    pub fn put_block(&self, block: Arc<block::Block>) -> Result<(), String> {
        let h = self.chain.block_height();
        let _queue_lock = self.queue_lock.write().unwrap();
        if self.discarded.load(Ordering::SeqCst) {
            return Ok(());
        }
        // Can easily happen when fetching the same blocks from
        // different peers, thus not considered as error.
        if block.index <= h {
            return Ok(());
        }
        if h + self.cache_size as u32 < block.index {
            match self.mode {
                OperationMode::NonBlocking => return Ok(()),
                OperationMode::Blocking => {
                    drop(_queue_lock);
                    let ticker = thread::spawn(move || {
                        loop {
                            thread::sleep(Duration::from_secs(1));
                            if self.discarded.load(Ordering::SeqCst) {
                                return Ok(());
                            }
                            let h = self.chain.block_height();
                            if h + self.cache_size as u32 >= block.index {
                                break;
                            }
                        }
                    });
                    ticker.join().unwrap();
                }
            }
        }
        let pos = self.index_to_position(block.index);
        // If we already have it, keep the old block, throw away the new one.
        if self.queue[pos].is_none() || self.queue[pos].as_ref().unwrap().index < block.index {
            *self.len.lock().unwrap() += 1;
            self.queue[pos] = Some(block);
            for pos in pos..self.cache_size {
                if self.queue[pos].is_some() && self.last_q + 1 == self.queue[pos].as_ref().unwrap().index {
                    self.last_q = self.queue[pos].as_ref().unwrap().index;
                }
            }
        }
        // update metrics
        if let Some(ref len_update_f) = self.len_update_f {
            len_update_f(*self.len.lock().unwrap());
        }
        if self.check_blocks.send(()).is_err() {
            // it's already busy processing blocks
        }
        Ok(())
    }

    // LastQueued returns the index of the last queued block and the queue's capacity
    // left.
    pub fn last_queued(&self) -> (u32, usize) {
        let _queue_lock = self.queue_lock.read().unwrap();
        (self.last_q, self.cache_size - *self.len.lock().unwrap())
    }

    // Discard stops the queue and prevents it from accepting more blocks to enqueue.
    pub fn discard(&self) {
        if self.discarded.compare_and_swap(false, true, Ordering::SeqCst) == false {
            let _queue_lock = self.queue_lock.write().unwrap();
            self.check_blocks.send(()).unwrap();
            self.queue.clear();
            *self.len.lock().unwrap() = 0;
        }
    }
}
