use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Notify};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use log::{info, error};
use sha2::{Sha256, Digest};
use url::Url;
use crate::config;
use crate::core::block;
use crate::services::oracle::neofs;
use crate::wallet;
use neofs_sdk::client;
use neofs_sdk::object;
use neofs_sdk::object::id::ID as OID;

const OID_SIZE: usize = 32;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
const DEFAULT_OID_BATCH_SIZE: usize = 8000;
const DEFAULT_DOWNLOADER_WORKERS_COUNT: usize = 100;

pub trait Ledger {
    fn get_config(&self) -> config::Blockchain;
    fn block_height(&self) -> u32;
}

pub struct Service {
    is_active: Arc<Mutex<bool>>,
    log: Arc<log::Logger>,
    cfg: config::NeoFSBlockFetcher,
    state_root_in_header: bool,

    chain: Arc<dyn Ledger>,
    client: Arc<Mutex<Option<client::Client>>>,
    enqueue_block: Arc<dyn Fn(Arc<block::Block>) -> Result<(), Box<dyn std::error::Error>> + Send + Sync>,
    account: Arc<wallet::Account>,

    oids_ch: mpsc::Sender<OID>,
    blocks_ch: mpsc::Sender<Arc<block::Block>>,
    wg: Arc<Notify>,

    ctx_cancel: Arc<Notify>,
    quit: Arc<Notify>,
    shutdown_callback: Arc<dyn Fn() + Send + Sync>,
}

impl Service {
    pub async fn new(
        chain: Arc<dyn Ledger>,
        cfg: config::NeoFSBlockFetcher,
        logger: Arc<log::Logger>,
        put_block: Arc<dyn Fn(Arc<block::Block>) -> Result<(), Box<dyn std::error::Error>> + Send + Sync>,
        shutdown_callback: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Arc<Self>, Box<dyn std::error::Error>> {
        let account = if !cfg.unlock_wallet.path.is_empty() {
            let wallet_from_file = wallet::Wallet::from_file(&cfg.unlock_wallet.path).await?;
            let account = wallet_from_file.accounts.iter()
                .find(|acc| acc.decrypt(&cfg.unlock_wallet.password, &wallet_from_file.scrypt).is_ok())
                .ok_or("failed to decrypt any account in the wallet")?;
            Arc::new(account.clone())
        } else {
            Arc::new(wallet::Account::new()?)
        };

        let (oids_tx, _) = mpsc::channel(2 * cfg.oid_batch_size.unwrap_or(DEFAULT_OID_BATCH_SIZE));
        let (blocks_tx, _) = mpsc::channel(cfg.oid_batch_size.unwrap_or(DEFAULT_OID_BATCH_SIZE));

        Ok(Arc::new(Service {
            is_active: Arc::new(Mutex::new(false)),
            log: logger,
            cfg,
            state_root_in_header: chain.get_config().state_root_in_header,
            chain,
            client: Arc::new(Mutex::new(None)),
            enqueue_block: put_block,
            account,
            oids_ch: oids_tx,
            blocks_ch: blocks_tx,
            wg: Arc::new(Notify::new()),
            ctx_cancel: Arc::new(Notify::new()),
            quit: Arc::new(Notify::new()),
            shutdown_callback,
        }))
    }

    pub async fn start(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error>> {
        let mut is_active = self.is_active.lock().unwrap();
        if *is_active {
            return Ok(());
        }
        *is_active = true;
        drop(is_active);

        info!(self.log, "starting NeoFS BlockFetcher service");

        let client = neofs::get_sdk_client(&self.cfg.addresses[0], Duration::from_secs(600)).await?;
        *self.client.lock().unwrap() = Some(client);

        let service = self.clone();
        tokio::spawn(async move {
            service.exiter().await;
        });

        let service = self.clone();
        tokio::spawn(async move {
            service.oid_downloader().await;
        });

        for _ in 0..self.cfg.downloader_workers_count.unwrap_or(DEFAULT_DOWNLOADER_WORKERS_COUNT) {
            let service = self.clone();
            self.wg.notify_one();
            tokio::spawn(async move {
                service.block_downloader().await;
            });
        }

        let service = self.clone();
        tokio::spawn(async move {
            service.block_queuer().await;
        });

        Ok(())
    }

    async fn oid_downloader(self: Arc<Self>) {
        let result = if self.cfg.skip_index_files_search {
            self.fetch_oids_by_search().await
        } else {
            self.fetch_oids_from_index_files().await
        };

        if let Err(err) = result {
            error!(self.log, "NeoFS BlockFetcher service: OID downloading routine failed: {:?}", err);
            self.stop_service(true).await;
        }
    }

    async fn block_downloader(self: Arc<Self>) {
        self.wg.notified().await;

        while let Some(blk_oid) = self.oids_ch.recv().await {
            let ctx = timeout(self.cfg.timeout.unwrap_or(DEFAULT_TIMEOUT), async {
                self.object_get(&blk_oid.to_string()).await
            }).await;

            match ctx {
                Ok(Ok(rc)) => {
                    match self.read_block(rc).await {
                        Ok(block) => {
                            if self.blocks_ch.send(block).await.is_err() {
                                break;
                            }
                        }
                        Err(err) => {
                            error!(self.log, "failed to read block: {:?}", err);
                            self.stop_service(true).await;
                            break;
                        }
                    }
                }
                Ok(Err(err)) => {
                    if is_context_canceled_err(&err) {
                        break;
                    }
                    error!(self.log, "failed to objectGet block: {:?}", err);
                    self.stop_service(true).await;
                    break;
                }
                Err(_) => {
                    break;
                }
            }
        }
    }

    async fn block_queuer(self: Arc<Self>) {
        while let Some(block) = self.blocks_ch.recv().await {
            if (self.enqueue_block)(block).is_err() {
                error!(self.log, "failed to enqueue block");
                self.stop_service(true).await;
                break;
            }
        }
    }

    async fn fetch_oids_from_index_files(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error>> {
        let mut start_index = self.chain.block_height() / self.cfg.index_file_size + 1;
        let mut skip = self.chain.block_height() % self.cfg.index_file_size;

        loop {
            let prm = client::PrmObjectSearch::new();
            let filters = object::SearchFilters::new();
            filters.add_filter(&self.cfg.index_file_attribute, &format!("{}", start_index), object::MatchStringEqual);
            prm.set_filters(filters);

            let ctx = timeout(self.cfg.timeout.unwrap_or(DEFAULT_TIMEOUT), async {
                self.object_search(prm).await
            }).await;

            match ctx {
                Ok(Ok(block_oids_object)) => {
                    if block_oids_object.is_empty() {
                        info!(self.log, "NeoFS BlockFetcher service: no '{}' object found with index {}, stopping", self.cfg.index_file_attribute, start_index);
                        break;
                    }

                    let oids_rc = self.object_get(&block_oids_object[0].to_string()).await?;
                    self.stream_block_oids(oids_rc, skip as usize).await?;
                    start_index += 1;
                    skip = 0;
                }
                Ok(Err(err)) => {
                    if is_context_canceled_err(&err) {
                        break;
                    }
                    return Err(Box::new(err));
                }
                Err(_) => {
                    break;
                }
            }
        }

        Ok(())
    }

    async fn stream_block_oids(self: Arc<Self>, mut rc: Box<dyn tokio::io::AsyncRead + Unpin + Send>, skip: usize) -> Result<(), Box<dyn std::error::Error>> {
        let mut oid_bytes = vec![0u8; OID_SIZE];
        let mut oids_processed = 0;

        loop {
            match tokio::io::AsyncReadExt::read_exact(&mut rc, &mut oid_bytes).await {
                Ok(_) => {
                    if oids_processed < skip {
                        oids_processed += 1;
                        continue;
                    }

                    let oid_block = OID::decode(&oid_bytes)?;
                    if self.oids_ch.send(oid_block).await.is_err() {
                        break;
                    }

                    oids_processed += 1;
                }
                Err(err) if err.kind() == tokio::io::ErrorKind::UnexpectedEof => {
                    break;
                }
                Err(err) => {
                    return Err(Box::new(err));
                }
            }
        }

        if oids_processed != self.cfg.index_file_size as usize {
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, format!("block OIDs count mismatch: expected {}, processed {}", self.cfg.index_file_size, oids_processed))));
        }

        Ok(())
    }

    async fn fetch_oids_by_search(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error>> {
        let mut start_index = self.chain.block_height();
        let batch_size = self.cfg.oid_batch_size.unwrap_or(DEFAULT_OID_BATCH_SIZE) as u32;

        loop {
            let prm = client::PrmObjectSearch::new();
            let filters = object::SearchFilters::new();
            filters.add_filter(&self.cfg.block_attribute, &format!("{}", start_index), object::MatchNumGE);
            filters.add_filter(&self.cfg.block_attribute, &format!("{}", start_index + batch_size - 1), object::MatchNumLE);
            prm.set_filters(filters);

            let ctx = timeout(self.cfg.timeout.unwrap_or(DEFAULT_TIMEOUT), async {
                self.object_search(prm).await
            }).await;

            match ctx {
                Ok(Ok(block_oids)) => {
                    if block_oids.is_empty() {
                        info!(self.log, "NeoFS BlockFetcher service: no block found with index {}, stopping", start_index);
                        break;
                    }

                    for oid in block_oids {
                        if self.oids_ch.send(oid).await.is_err() {
                            break;
                        }
                    }

                    start_index += batch_size;
                }
                Ok(Err(err)) => {
                    if is_context_canceled_err(&err) {
                        break;
                    }
                    return Err(Box::new(err));
                }
                Err(_) => {
                    break;
                }
            }
        }

        Ok(())
    }

    async fn read_block(self: Arc<Self>, mut rc: Box<dyn tokio::io::AsyncRead + Unpin + Send>) -> Result<Arc<block::Block>, Box<dyn std::error::Error>> {
        let mut b = block::Block::new(self.state_root_in_header);
        let mut r = gio::BinReader::new(rc);
        b.decode_binary(&mut r).await?;
        Ok(Arc::new(b))
    }

    pub async fn shutdown(self: Arc<Self>) {
        if !self.is_active() {
            return;
        }
        self.stop_service(true).await;
        self.quit.notified().await;
    }

    async fn stop_service(self: Arc<Self>, force: bool) {
        let mut is_active = self.is_active.lock().unwrap();
        if !*is_active {
            return;
        }
        *is_active = false;
        drop(is_active);

        info!(self.log, "shutting down NeoFS BlockFetcher service", force);

        if force {
            self.ctx_cancel.notify_waiters();
        }

        self.quit.notify_waiters();
        self.wg.notified().await;

        self.shutdown_callback();
    }

    pub fn is_active(&self) -> bool {
        *self.is_active.lock().unwrap()
    }

    async fn object_get(&self, oid: &str) -> Result<Box<dyn tokio::io::AsyncRead + Unpin + Send>, Box<dyn std::error::Error>> {
        let url = Url::parse(&format!("neofs:{}{}", self.cfg.container_id, oid))?;
        let rc = neofs::get_with_client(&self.client.lock().unwrap().as_ref().unwrap(), &self.account.private_key(), &url, false).await?;
        Ok(Box::new(rc))
    }

    async fn object_search(&self, prm: client::PrmObjectSearch) -> Result<Vec<OID>, Box<dyn std::error::Error>> {
        neofs::object_search(&self.client.lock().unwrap().as_ref().unwrap(), &self.account.private_key(), &self.cfg.container_id, prm).await
    }
}

fn is_context_canceled_err(err: &Box<dyn std::error::Error>) -> bool {
    err.to_string().contains("context canceled")
}