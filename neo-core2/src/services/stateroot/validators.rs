use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::sleep;

use neo_core2::core::state::MPTRoot;
use neo_core2::core::transaction::Witness;
use neo_core2::io::BufBinWriter;
use neo_core2::network::payload::Extensible;
use neo_core2::vm::emit;
use neo_core2::wallet::Account;

const VOTE_VALID_END_INC: u32 = 10;
const FIRST_VOTE_RESEND_DELAY: Duration = Duration::from_secs(3);

pub struct Service {
    chain: Arc<dyn Chain>,
    network: Network,
    main_cfg: MainConfig,
    log: slog::Logger,
    started: AtomicBool,
    block_rx: mpsc::Receiver<Block>,
    stop_tx: mpsc::Sender<()>,
    done_rx: mpsc::Receiver<()>,
    wallet: Option<Wallet>,
    incomplete_roots: Arc<RwLock<HashMap<u32, Arc<Mutex<IncompleteRoot>>>>>,
    acc: RwLock<(u8, Option<Account>)>,
    max_retries: u32,
    time_per_block: Duration,
}

impl Service {
    pub fn name(&self) -> &'static str {
        "stateroot"
    }

    pub fn start(&self) {
        if !self.started.compare_and_swap(false, true, Ordering::SeqCst) {
            return;
        }
        self.log.info("starting state validation service");
        tokio::spawn(self.run());
    }

    async fn run(&self) {
        self.chain.subscribe_for_blocks(self.block_rx.clone());

        loop {
            tokio::select! {
                Some(b) = self.block_rx.recv() => {
                    match self.get_state_root(b.index()).await {
                        Ok(r) => {
                            if let Err(e) = self.sign_and_send(&r).await {
                                self.log.error("can't sign or send state root", slog::o!("error" => e.to_string()));
                            }
                        },
                        Err(e) => {
                            self.log.error("can't get state root for new block", slog::o!("error" => e.to_string()));
                        }
                    }
                    self.incomplete_roots.write().await.remove(&(b.index() - VOTE_VALID_END_INC));
                },
                _ = self.stop_tx.closed() => break,
            }
        }

        self.chain.unsubscribe_from_blocks(&self.block_rx);
        self.block_rx.close();
        self.done_rx.close();
    }

    pub async fn shutdown(&self) {
        if !self.started.compare_and_swap(true, false, Ordering::SeqCst) {
            return;
        }
        self.log.info("stopping state validation service");
        self.stop_tx.send(()).await.unwrap();
        self.done_rx.recv().await;
        if let Some(wallet) = &self.wallet {
            wallet.close().await;
        }
        self.log.sync();
    }

    async fn sign_and_send(&self, r: &MPTRoot) -> Result<(), Box<dyn std::error::Error>> {
        if !self.main_cfg.enabled {
            return Ok(());
        }

        let (my_index, acc) = self.get_account();
        let acc = match acc {
            Some(a) => a,
            None => return Ok(()),
        };

        let sig = acc.sign_hashable(&self.network, r);
        let inc_root = self.get_incomplete_root(r.index, my_index).await;
        let mut inc_root = inc_root.lock().await;
        inc_root.root = r.clone();
        inc_root.add_signature(acc.public_key(), sig.clone());
        inc_root.reverify(&self.network);
        self.try_send_root(&inc_root, &acc).await;

        let msg = Message::new(VoteT, Vote {
            validator_index: my_index as i32,
            height: r.index,
            signature: sig,
        });

        let mut w = BufBinWriter::new();
        msg.encode_binary(&mut w)?;

        let e = Extensible {
            category: Category,
            valid_block_start: r.index,
            valid_block_end: r.index + VOTE_VALID_END_INC,
            sender: acc.script_hash(),
            data: w.to_vec(),
            witness: Witness {
                verification_script: acc.get_verification_script(),
                invocation_script: Vec::new(),
            },
        };

        let sig = acc.sign_hashable(&self.network, &e);
        let mut buf = BufBinWriter::new();
        emit::bytes(&mut buf, &sig)?;
        e.witness.invocation_script = buf.to_vec();

        inc_root.my_vote = Some(e);
        inc_root.retries = -1;
        self.send_vote(&mut inc_root).await;

        Ok(())
    }

    async fn send_vote(&self, ir: &mut IncompleteRoot) {
        if ir.is_sent || ir.retries >= self.max_retries as i32 || 
           self.chain.header_height().await >= ir.my_vote.as_ref().unwrap().valid_block_end {
            return;
        }

        self.relay_extensible(ir.my_vote.as_ref().unwrap()).await;

        let delay = if ir.retries > 0 {
            self.time_per_block * (1 << ir.retries)
        } else {
            FIRST_VOTE_RESEND_DELAY
        };

        let ir_clone = Arc::clone(&ir);
        let self_clone = Arc::clone(&self);
        tokio::spawn(async move {
            sleep(delay).await;
            let mut ir = ir_clone.lock().await;
            self_clone.send_vote(&mut ir).await;
        });

        ir.retries += 1;
    }

    fn get_account(&self) -> (u8, Option<Account>) {
        let guard = self.acc.read().unwrap();
        (guard.0, guard.1.clone())
    }
}
