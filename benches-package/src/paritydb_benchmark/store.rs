use super::{PARITY_DB_CRATE_VERSION, ParityDbConfigurationReport, ParityDbStageTotals};
use crate::storage_workload::{
    MPT_NODE_KEY_BYTES, MPT_NODE_PREFIX, OperationKind, WorkloadOperation,
};
use anyhow::{Context, Result, ensure};
use parity_db::{ColumnOptions, CompressionType, Db, Options};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const NODE_COLUMN: u8 = 0;
const NODE_HASH_BYTES: usize = 32;

pub(super) struct ParityDbStore {
    db: Option<Db>,
    root: PathBuf,
}

impl ParityDbStore {
    pub(super) fn create(root: &Path) -> Result<Self> {
        ensure_fresh_directory(root)?;
        let db = Db::open_or_create(&database_options(root))
            .with_context(|| format!("create ParityDB database {}", root.display()))?;
        Ok(Self {
            db: Some(db),
            root: root.to_path_buf(),
        })
    }

    pub(super) fn open_read_only(root: &Path) -> Result<Self> {
        let db = Db::open_read_only(&database_options(root))
            .with_context(|| format!("open ParityDB read-only {}", root.display()))?;
        Ok(Self {
            db: Some(db),
            root: root.to_path_buf(),
        })
    }

    pub(super) fn configuration() -> ParityDbConfigurationReport {
        ParityDbConfigurationReport {
            crate_version: PARITY_DB_CRATE_VERSION.to_owned(),
            column: NODE_COLUMN,
            stored_key_bytes: NODE_HASH_BYTES,
            implicit_namespace: MPT_NODE_PREFIX,
            uniform: true,
            preimage: false,
            ref_counted: false,
            compression: "none".to_owned(),
            btree_index: false,
            sync_wal: true,
            sync_data: true,
            background_threads: true,
            durability_fence: "supported API: commit -> drop/drain queued user commit and WAL work (reindex may remain) -> open -> verify one exact transaction sentinel".to_owned(),
        }
    }

    /// Commits one logical batch, closes the writer to drain its asynchronous
    /// pipeline, reopens it, and verifies a value changed by the transaction.
    pub(super) fn commit_durable(
        &mut self,
        operations: &[WorkloadOperation],
    ) -> Result<ParityDbStageTotals> {
        ensure!(!operations.is_empty(), "ParityDB commit batch is empty");
        let mut newest = BTreeMap::new();
        for operation in operations {
            let key = node_hash(&operation.key)?;
            let value = match &operation.kind {
                OperationKind::Put(value) => Some(value.clone()),
                OperationKind::Tombstone => None,
            };
            newest.insert(key, value);
        }
        let sentinel = if let Some(sentinel) = newest
            .iter()
            .rev()
            .find_map(|(key, value)| value.as_ref().map(|value| (*key, Some(value.clone()))))
        {
            sentinel
        } else {
            let mut sentinel = None;
            for key in newest.keys() {
                if self.db()?.get(NODE_COLUMN, key)?.is_some() {
                    sentinel = Some((*key, None));
                    break;
                }
            }
            sentinel.context(
                "all-tombstone ParityDB transaction has no present key to verify after reopen",
            )?
        };
        let transaction = newest
            .into_iter()
            .map(|(key, value)| (NODE_COLUMN, key, value))
            .collect::<Vec<_>>();

        let mut totals = ParityDbStageTotals::default();
        let started = Instant::now();
        self.db()?
            .commit(transaction)
            .context("admit ParityDB transaction")?;
        totals.commit_enqueue_ns = duration_ns(started.elapsed());

        let started = Instant::now();
        let db = self
            .db
            .take()
            .context("ParityDB writer is unavailable before durability close")?;
        drop(db);
        totals.close_drain_ns = duration_ns(started.elapsed());

        let started = Instant::now();
        let reopened = Db::open(&database_options(&self.root))
            .with_context(|| format!("reopen ParityDB writer {}", self.root.display()))?;
        totals.reopen_ns = duration_ns(started.elapsed());
        self.db = Some(reopened);

        let started = Instant::now();
        let actual = self
            .db()?
            .get(NODE_COLUMN, &sentinel.0)
            .context("verify ParityDB transaction sentinel after reopen")?;
        ensure!(
            actual == sentinel.1,
            "ParityDB transaction sentinel differs after close/reopen"
        );
        totals.post_reopen_verify_ns = duration_ns(started.elapsed());
        totals.durable_fences = 1;
        Ok(totals)
    }

    pub(super) fn get(&self, key: &[u8; MPT_NODE_KEY_BYTES]) -> Result<Option<Vec<u8>>> {
        if key[0] != MPT_NODE_PREFIX {
            return Ok(None);
        }
        self.db()?
            .get(NODE_COLUMN, &node_hash(key)?)
            .context("read ParityDB node hash")
    }

    pub(super) fn get_many_sorted(
        &self,
        keys: &[[u8; MPT_NODE_KEY_BYTES]],
    ) -> Result<Vec<Option<Vec<u8>>>> {
        ensure!(
            keys.windows(2).all(|pair| pair[0] <= pair[1]),
            "ParityDB sorted lookup keys are not ordered"
        );
        keys.iter().map(|key| self.get(key)).collect()
    }

    pub(super) fn value_entries(&self) -> Result<Option<u64>> {
        match self.db()?.get_num_column_value_entries(NODE_COLUMN) {
            Ok(entries) => Ok(Some(entries)),
            Err(parity_db::Error::InvalidConfiguration(message))
                if message.contains("Unable to determine number") =>
            {
                Ok(None)
            }
            Err(error) => Err(error).context("read ParityDB value entry count"),
        }
    }

    pub(super) fn hash_index_files(&self) -> Result<Vec<String>> {
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("read ParityDB layout {}", self.root.display()))?
        {
            let entry = entry.context("read ParityDB layout entry")?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name
                .strip_prefix("index_00_")
                .is_some_and(|bits| bits.parse::<u8>().is_ok())
                && entry
                    .file_type()
                    .context("read ParityDB index file type")?
                    .is_file()
            {
                files.push(name);
            }
        }
        files.sort_unstable();
        ensure!(!files.is_empty(), "ParityDB node hash index file is absent");
        Ok(files)
    }

    fn db(&self) -> Result<&Db> {
        self.db
            .as_ref()
            .context("ParityDB handle is unavailable during close/reopen")
    }
}

fn database_options(root: &Path) -> Options {
    let mut options = Options::with_columns(root, 1);
    options.columns[usize::from(NODE_COLUMN)] = ColumnOptions {
        preimage: false,
        uniform: true,
        ref_counted: false,
        compression: CompressionType::NoCompression,
        btree_index: false,
        multitree: false,
        append_only: false,
        allow_direct_node_access: false,
    };
    options.sync_wal = true;
    options.sync_data = true;
    options.stats = false;
    options
}

fn ensure_fresh_directory(root: &Path) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(root)
        .with_context(|| format!("inspect ParityDB path {}", root.display()))?;
    ensure!(metadata.is_dir(), "ParityDB path is not a directory");
    ensure!(
        fs::read_dir(root)
            .with_context(|| format!("read ParityDB directory {}", root.display()))?
            .next()
            .is_none(),
        "ParityDB benchmark requires a fresh empty directory"
    );
    Ok(())
}

fn node_hash(key: &[u8; MPT_NODE_KEY_BYTES]) -> Result<[u8; NODE_HASH_BYTES]> {
    ensure!(
        key[0] == MPT_NODE_PREFIX,
        "ParityDB workload key is outside the MPT node namespace"
    );
    key[1..]
        .try_into()
        .context("MPT node hash does not contain 32 bytes")
}

fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn operation(key_byte: u8, kind: OperationKind) -> WorkloadOperation {
        let mut key = [0u8; MPT_NODE_KEY_BYTES];
        key[0] = MPT_NODE_PREFIX;
        key[1] = key_byte;
        WorkloadOperation {
            key,
            kind,
            version_hit: false,
        }
    }

    #[test]
    fn supported_close_reopen_fences_preserve_replacement_and_tombstone() {
        let root = tempdir().expect("temporary root");
        let database = root.path().join("paritydb");
        let mut store = ParityDbStore::create(&database).expect("create store");
        let put = operation(1, OperationKind::Put(vec![1, 2, 3]));
        store
            .commit_durable(std::slice::from_ref(&put))
            .expect("commit first put");
        let updated = operation(1, OperationKind::Put(vec![4, 5]));
        store
            .commit_durable(std::slice::from_ref(&updated))
            .expect("commit updated put");
        assert_eq!(
            store.get(&updated.key).expect("read update"),
            Some(vec![4, 5])
        );
        let tombstone = operation(1, OperationKind::Tombstone);
        store
            .commit_durable(std::slice::from_ref(&tombstone))
            .expect("commit tombstone");
        assert_eq!(store.get(&tombstone.key).expect("read tombstone"), None);
        drop(store);

        let reopened = ParityDbStore::open_read_only(&database).expect("reopen store");
        assert_eq!(reopened.get(&tombstone.key).expect("read reopened"), None);
    }

    #[test]
    fn keys_outside_the_implicit_namespace_are_absent() {
        let root = tempdir().expect("temporary root");
        let database = root.path().join("paritydb");
        let mut store = ParityDbStore::create(&database).expect("create store");
        let put = operation(7, OperationKind::Put(vec![1, 2, 3]));
        store
            .commit_durable(std::slice::from_ref(&put))
            .expect("commit put");

        let mut missing = put.key;
        missing[0] = MPT_NODE_PREFIX.wrapping_add(1);
        assert_eq!(store.get(&missing).expect("read implicit miss"), None);
        assert_eq!(
            store.get(&put.key).expect("read stored node"),
            Some(vec![1, 2, 3])
        );
    }
}
