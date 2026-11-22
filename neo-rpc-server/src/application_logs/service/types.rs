use super::*;
/// Serialized bytes an import session may hold before it commits on its own.
///
/// One checkpoint interval can cover tens of thousands of blocks, and a
/// transaction-bearing block can carry a large log. The cap bounds the memory a
/// session pins between two checkpoints without changing what is stored: an
/// automatic commit writes exactly the records the next checkpoint would have
/// written.
pub const DEFAULT_ACCEPTED_BYTE_CAP: u64 = 64 * 1024 * 1024;

/// One application-log record staged inside a persistence unit.
///
/// The key and JSON value are exactly what a standalone block would have put
/// into the logs snapshot, so promotion cannot change the stored bytes.
pub(super) struct StagedLog {
    pub(super) key: Vec<u8>,
    pub(super) value: Vec<u8>,
}

impl StagedLog {
    /// Serialized size this record occupies in the logs database.
    pub(super) fn byte_len(&self) -> u64 {
        u64::try_from(self.key.len().saturating_add(self.value.len())).unwrap_or(u64::MAX)
    }
}

/// Records held by the persistence unit that is currently open.
///
/// `staged` holds records written by a committing callback whose committed
/// callback has not fired yet. `committed` holds records whose committed
/// callback already fired inside this unit. Neither list has reached the logs
/// database, so a chain-commit failure can still discard both.
///
/// `watermark` is the newest block the unit covers. It travels with the records
/// so a commit can report the exact height its bytes make durable.
#[derive(Default)]
pub(super) struct OpenUnit {
    pub(super) staged: Vec<StagedLog>,
    pub(super) committed: Vec<StagedLog>,
    pub(super) watermark: Option<(u32, UInt256)>,
}

/// Outcome of an import-scoped persistence-unit lifecycle call.
///
/// The lifecycle is defensive by construction: a repeated `begin` keeps the
/// unit that is already open, and `finish`/`abort` without an open unit change
/// nothing. Callers can therefore react to misuse instead of unwinding a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceUnitOutcome {
    /// `begin` opened a new unit.
    Started,
    /// `finish` promoted this many staged records to accepted-pending.
    Finished {
        /// Records the finished unit handed to `flush_accepted`.
        records: usize,
    },
    /// `abort` discarded this many staged records.
    Aborted {
        /// Records dropped from the unit that was open.
        records: usize,
    },
    /// `begin` while a unit was already open; the open unit is untouched.
    AlreadyOpen,
    /// `finish`/`abort` without an open unit; no record was changed.
    NotOpen,
}

/// Outcome of one accepted-record commit.
///
/// `watermark` is the block watermark the committed bytes cover. The bytes sit
/// in the backend, not on disk, until
/// [`ApplicationLogsService::try_flush_durable`] confirms the flush, so a caller
/// must not certify a checkpoint from this value alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AcceptedFlush {
    /// Records written by this commit.
    pub records: u64,
    /// Serialized bytes those records occupy.
    pub bytes: u64,
    /// Highest block whose records a commit has written, if a block boundary was
    /// promoted with them. Durability requires the separate fallible flush.
    pub watermark: Option<(u32, UInt256)>,
}

/// Logs persistence telemetry of one import session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LogsFlushTelemetry {
    /// Commits that wrote at least one record.
    pub flushes: u64,
    /// Records those commits wrote.
    pub records: u64,
    /// Serialized bytes those commits wrote.
    pub bytes: u64,
    /// Records still waiting for a commit.
    pub pending_records: u64,
    /// Highest height whose records are durable.
    pub watermark: Option<(u32, UInt256)>,
}

/// Outcome of an import-session lifecycle call.
///
/// Like [`PersistenceUnitOutcome`], the lifecycle is defensive: a repeated
/// `begin` keeps the session that is already open, and `end` without a session
/// flushes nothing. Callers react to misuse instead of unwinding a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSessionOutcome {
    /// `begin` opened a session with a fresh watermark and flush counters.
    Started,
    /// `begin` while a session was already open; its state is untouched.
    AlreadyOpen,
    /// `end` committed the records still pending and left batching mode.
    Ended {
        /// Records the closing commit wrote.
        records: u64,
        /// Serialized bytes the closing commit wrote.
        bytes: u64,
    },
    /// `end` without an open session; nothing was written.
    NotOpen,
}

/// Records promoted by finished persistence units plus the import session state.
///
/// Online operation never promotes a record, so every field but `byte_cap`
/// keeps its default unless an import session is open.
pub(super) struct AcceptedLogs {
    /// Records a finished unit promoted and no commit has written yet.
    pub(super) records: Vec<StagedLog>,
    /// Serialized size of `records`.
    pub(super) pending_bytes: u64,
    /// Newest block covered by `records`.
    pub(super) pending_watermark: Option<(u32, UInt256)>,
    /// Newest block whose records a commit wrote but no durable flush confirmed.
    ///
    /// The import profile disables the write-ahead log, so bytes accepted by a
    /// commit are still lost on a crash until a durable flush succeeds. Keeping
    /// this stage separate from `durable_watermark` is what stops a checkpoint
    /// from certifying records that never reached disk.
    pub(super) committed_watermark: Option<(u32, UInt256)>,
    /// Newest block whose records are durable in the logs database.
    pub(super) durable_watermark: Option<(u32, UInt256)>,
    /// Whether [`ApplicationLogsService::begin_import_session`] is in effect.
    pub(super) session_active: bool,
    /// Automatic commit threshold in serialized bytes; zero commits every unit.
    pub(super) byte_cap: u64,
    /// Commits that wrote at least one record.
    pub(super) flushes: u64,
    /// Records those commits wrote.
    pub(super) flushed_records: u64,
    /// Serialized bytes those commits wrote.
    pub(super) flushed_bytes: u64,
}

impl Default for AcceptedLogs {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            pending_bytes: 0,
            pending_watermark: None,
            committed_watermark: None,
            durable_watermark: None,
            session_active: false,
            byte_cap: DEFAULT_ACCEPTED_BYTE_CAP,
            flushes: 0,
            flushed_records: 0,
            flushed_bytes: 0,
        }
    }
}

impl AcceptedLogs {
    /// Promotes the records of a finished unit and returns their byte size.
    ///
    /// A unit's blocks are always newer than what is already pending, so its
    /// watermark replaces the pending one instead of being merged.
    pub(super) fn push(
        &mut self,
        records: Vec<StagedLog>,
        watermark: Option<(u32, UInt256)>,
    ) -> u64 {
        let bytes = records.iter().map(StagedLog::byte_len).sum();
        self.pending_bytes = self.pending_bytes.saturating_add(bytes);
        self.records.extend(records);
        if watermark.is_some() {
            self.pending_watermark = watermark;
        }
        bytes
    }
}
