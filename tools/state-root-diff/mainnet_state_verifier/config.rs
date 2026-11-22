use super::*;
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum ExecutionModeArg {
    Canonical,
    Auto,
    Verify,
}

impl From<ExecutionModeArg> for ExecutionMode {
    fn from(value: ExecutionModeArg) -> Self {
        match value {
            ExecutionModeArg::Canonical => Self::Canonical,
            ExecutionModeArg::Auto => Self::Auto,
            ExecutionModeArg::Verify => Self::Verify,
        }
    }
}

#[derive(Debug, Parser)]
#[command(about = "Verify every MainNet block against two independent RPC upstreams")]
pub(super) struct Args {
    /// Read an uncompressed official chain.N.acc file and replay it without RPC upstreams.
    /// Offline mode proves local execution only; it does not claim cross-implementation parity.
    #[arg(long)]
    pub(super) offline_package: Option<PathBuf>,
    /// Explicitly reconstruct a short missing audit tail from the package and persisted roots.
    #[arg(long)]
    pub(super) repair_offline_audit: bool,
    /// Stop cleanly after this many seconds at an offline batch boundary.
    #[arg(long)]
    pub(super) offline_run_seconds: Option<u64>,
    #[arg(long, default_value = "http://seed1.neo.org:10332")]
    pub(super) primary: String,
    #[arg(long, default_value = "http://seed2.neo.org:10332")]
    pub(super) secondary: String,
    #[arg(long, default_value = "reports/mainnet/full-validation/store")]
    pub(super) data_path: PathBuf,
    #[arg(long, default_value = "reports/mainnet/full-validation/verified.jsonl")]
    pub(super) audit_path: PathBuf,
    /// Inclusive target height. Defaults to the common upstream tip at startup.
    #[arg(long)]
    pub(super) end: Option<u32>,
    #[arg(long, default_value_t = 32)]
    pub(super) batch_size: u32,
    #[arg(long, value_enum, default_value_t = ExecutionModeArg::Canonical)]
    pub(super) execution_mode: ExecutionModeArg,
    /// Also compare OnPersist/PostPersist/transaction receipts from both upstreams with neo-rs.
    #[arg(long)]
    pub(super) verify_receipts: bool,
    /// Disable bounded next-batch prefetch; intended for serial A/B profiling.
    #[arg(long)]
    pub(super) no_prefetch: bool,
    /// Measure internal persistence stages without changing execution semantics.
    #[arg(long)]
    pub(super) profile_execution: bool,
    /// Keep WAL enabled, sync once per batch, then flush that batch's audit records.
    /// Interruptions between batch boundaries are rejected on restart.
    #[arg(long)]
    pub(super) batch_durable: bool,
    /// Disable the flat write-back cache for consecutive empty offline blocks.
    /// Intended for correctness differential tests and serial A/B profiling.
    #[arg(long)]
    pub(super) disable_empty_block_batch: bool,
    /// Bypass the MPT compact-node cache for a cold, offline A/B run.
    #[arg(long)]
    pub(super) disable_mpt_node_cache: bool,
    /// Reset the MPT cache to this nonzero capacity before replay (cold A/B only).
    #[arg(long)]
    pub(super) mpt_node_cache_capacity: Option<std::num::NonZeroUsize>,
    /// Allow an empty audit file to start at an already verified store checkpoint.
    /// The checkpoint block and state root are still checked against both upstreams.
    #[arg(long)]
    pub(super) new_audit_at_checkpoint: bool,
    #[arg(long, default_value_t = 45)]
    pub(super) timeout_seconds: u64,
    #[arg(long, default_value_t = 256 * 1024 * 1024)]
    pub(super) max_response_bytes: usize,
}

#[derive(Clone)]
pub(super) struct RpcEndpoint {
    name: &'static str,
    url: String,
    client: reqwest::Client,
    next_id: Arc<AtomicU64>,
    max_response_bytes: usize,
}

impl RpcEndpoint {
    pub(super) fn new(
        name: &'static str,
        url: String,
        timeout: Duration,
        max_response_bytes: usize,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .context("failed to construct RPC client")?;
        Ok(Self {
            name,
            url,
            client,
            next_id: Arc::new(AtomicU64::new(1)),
            max_response_bytes,
        })
    }

    pub(super) async fn call_one(&self, method: &'static str, params: Value) -> Result<Value> {
        let mut values = self
            .call_batch(vec![("one".to_owned(), method, params)])
            .await?;
        Ok(values
            .remove("one")
            .expect("call_batch preserves every requested key"))
    }

    async fn call_batch(
        &self,
        calls: Vec<(String, &'static str, Value)>,
    ) -> Result<HashMap<String, Value>> {
        let mut last_error = None;
        for attempt in 0..4 {
            match self.call_batch_once(&calls).await {
                Ok(values) => return Ok(values),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < 3 {
                        tokio::time::sleep(Duration::from_millis(250 * (attempt + 1))).await;
                    }
                }
            }
        }
        Err(last_error.expect("at least one RPC attempt was made"))
            .with_context(|| format!("{} RPC batch failed after four attempts", self.name))
    }

    async fn call_batch_once(
        &self,
        calls: &[(String, &'static str, Value)],
    ) -> Result<HashMap<String, Value>> {
        let mut expected = HashMap::with_capacity(calls.len());
        let mut request = Vec::with_capacity(calls.len());
        for (key, method, params) in calls {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            expected.insert(id, (key.clone(), *method));
            request.push(json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}));
        }
        let response = self
            .client
            .post(&self.url)
            .json(&request)
            .send()
            .await
            .with_context(|| format!("{} HTTP request failed", self.name))?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > self.max_response_bytes as u64)
        {
            bail!("{} response exceeds configured byte limit", self.name);
        }
        let bytes = response
            .bytes()
            .await
            .with_context(|| format!("{} response body read failed", self.name))?;
        if bytes.len() > self.max_response_bytes {
            bail!("{} response exceeds configured byte limit", self.name);
        }
        if !status.is_success() {
            bail!("{} returned HTTP {status}", self.name);
        }
        let payload: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("{} returned invalid JSON", self.name))?;
        parse_batch_response(payload, &expected)
            .with_context(|| format!("{} returned an invalid RPC batch", self.name))
    }

    pub(super) async fn block_count(&self) -> Result<u32> {
        strict_u32(
            &self.call_one("getblockcount", json!([])).await?,
            "block count",
        )
    }

    pub(super) async fn evidence(
        &self,
        start: u32,
        end: u32,
        verify_receipts: bool,
    ) -> Result<Vec<UpstreamEvidence>> {
        let mut calls = Vec::with_capacity(((end - start + 1) * 2) as usize);
        for height in start..=end {
            calls.push((
                format!("block:{height}"),
                "getblock",
                json!([height, false]),
            ));
            calls.push((format!("root:{height}"), "getstateroot", json!([height])));
        }
        let mut values = self.call_batch(calls).await?;
        let mut evidence = (start..=end)
            .map(|height| {
                let encoded = values
                    .remove(&format!("block:{height}"))
                    .context("block response missing")?;
                let encoded = encoded.as_str().context("getblock result is not Base64")?;
                let raw_block = general_purpose::STANDARD
                    .decode(encoded)
                    .context("getblock result is not canonical Base64")?;
                let root = values
                    .remove(&format!("root:{height}"))
                    .context("state-root response missing")?;
                let root_height = strict_u32(
                    root.get("index").context("state-root index missing")?,
                    "state-root index",
                )?;
                if root_height != height {
                    bail!("state-root index {root_height} does not match requested {height}");
                }
                let state_root =
                    parse_hash(root.get("roothash").and_then(Value::as_str), "state root")?;
                Ok(UpstreamEvidence {
                    height,
                    raw_block,
                    state_root,
                    executions: None,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if verify_receipts {
            let decoded = evidence
                .iter()
                .map(|item| {
                    let block = decode_block(&item.raw_block, item.height)?;
                    let block_hash = block.hash();
                    let transaction_hashes = block
                        .transactions
                        .iter()
                        .map(|transaction| transaction.hash())
                        .collect::<Vec<_>>();
                    Ok((block_hash, transaction_hashes))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut calls = Vec::new();
            for (item, (block_hash, transaction_hashes)) in evidence.iter().zip(&decoded) {
                calls.push((
                    format!("log:{}", item.height),
                    "getapplicationlog" as &'static str,
                    json!([block_hash.to_string()]),
                ));
                for (index, transaction_hash) in transaction_hashes.iter().enumerate() {
                    calls.push((
                        format!("tx:{}:{index}", item.height),
                        "getapplicationlog" as &'static str,
                        json!([transaction_hash.to_string()]),
                    ));
                }
            }
            let mut logs = self.call_batch(calls).await?;
            for (item, (block_hash, transaction_hashes)) in evidence.iter_mut().zip(decoded) {
                let log = logs
                    .remove(&format!("log:{}", item.height))
                    .context("block application-log response missing")?;
                let system = parse_application_log(&log, block_hash)?;
                if system.len() != 2
                    || system[0].trigger != "OnPersist"
                    || system[1].trigger != "PostPersist"
                {
                    bail!(
                        "block {} system log must contain exactly OnPersist and PostPersist",
                        item.height
                    );
                }
                let mut executions = Vec::with_capacity(system.len() + transaction_hashes.len());
                executions.push(system[0].clone());
                for (index, transaction_hash) in transaction_hashes.iter().enumerate() {
                    let log = logs
                        .remove(&format!("tx:{}:{index}", item.height))
                        .context("transaction application-log response missing")?;
                    let transaction = parse_transaction_log(&log, *transaction_hash)?;
                    if transaction.len() != 1 || transaction[0].trigger != "Application" {
                        bail!(
                            "transaction {} in block {} must have exactly one Application receipt",
                            transaction_hash,
                            item.height
                        );
                    }
                    executions.extend(transaction);
                }
                executions.push(system[1].clone());
                item.executions = Some(executions);
            }
        }
        Ok(evidence)
    }
}
