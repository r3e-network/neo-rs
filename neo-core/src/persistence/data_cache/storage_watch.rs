use crate::persistence::track_state::TrackState;
use crate::smart_contract::{StorageItem, StorageKey};
#[cfg(feature = "runtime")]
use crate::persistence::StorageItemExt;

#[cfg(feature = "runtime")]
use crate::{UInt160, UInt256};
#[cfg(feature = "runtime")]
use std::cell::RefCell;
#[cfg(feature = "runtime")]
use std::sync::OnceLock;
#[cfg(feature = "runtime")]
use tracing::{info, warn};

#[cfg(feature = "runtime")]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub(crate) enum StorageWatchPhase {
    #[default]
    Unknown,
    OnPersist,
    Application,
    PostPersist,
}

#[cfg(feature = "runtime")]
impl StorageWatchPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::OnPersist => "on_persist",
            Self::Application => "application",
            Self::PostPersist => "post_persist",
        }
    }
}

#[cfg(feature = "runtime")]
#[derive(Clone, Copy, Debug, Default)]
struct StorageWatchContext {
    block_index: u32,
    phase: StorageWatchPhase,
    tx_hash: Option<UInt256>,
}

#[cfg(feature = "runtime")]
thread_local! {
    static STORAGE_WATCH_CONTEXT: RefCell<Option<StorageWatchContext>> = const { RefCell::new(None) };
}

#[cfg(feature = "runtime")]
pub(crate) fn set_storage_watch_context(
    block_index: u32,
    phase: StorageWatchPhase,
    tx_hash: Option<UInt256>,
) {
    STORAGE_WATCH_CONTEXT.with(|context| {
        *context.borrow_mut() = Some(StorageWatchContext {
            block_index,
            phase,
            tx_hash,
        });
    });
}

#[cfg(feature = "runtime")]
pub(crate) fn clear_storage_watch_context() {
    STORAGE_WATCH_CONTEXT.with(|context| {
        *context.borrow_mut() = None;
    });
}

#[cfg(feature = "runtime")]
fn current_storage_watch_context() -> Option<StorageWatchContext> {
    STORAGE_WATCH_CONTEXT.with(|context| *context.borrow())
}

#[cfg(feature = "runtime")]
fn watched_gas_account_bytes() -> Option<([u8; 20], [u8; 20])> {
    static WATCHED: OnceLock<Option<([u8; 20], [u8; 20])>> = OnceLock::new();
    *WATCHED.get_or_init(|| {
        let raw = std::env::var("NEO_GAS_WATCH_ACCOUNT").ok()?;
        let normalized = raw.trim();
        match UInt160::parse(normalized) {
            Ok(account) => {
                let little_endian = account.as_bytes();
                let mut big_endian = little_endian;
                big_endian.reverse();
                Some((little_endian, big_endian))
            }
            Err(err) => {
                warn!(
                    target: "neo",
                    account = normalized,
                    error = %err,
                    "failed to parse NEO_GAS_WATCH_ACCOUNT for storage tracing"
                );
                None
            }
        }
    })
}

#[cfg(feature = "runtime")]
fn is_watched_gas_balance_key(key: &StorageKey) -> bool {
    const GAS_TOKEN_ID: i32 = -6;
    const ACCOUNT_PREFIX: u8 = 0x14;

    let Some((account_le, account_be)) = watched_gas_account_bytes() else {
        return false;
    };

    let key_account = &key.key()[1..];
    key.id() == GAS_TOKEN_ID
        && key.key().len() == 21
        && key.key()[0] == ACCOUNT_PREFIX
        && (key_account == account_le || key_account == account_be)
}

#[cfg(feature = "runtime")]
pub(super) fn log_watched_storage_event(
    op: &'static str,
    source: &'static str,
    key: &StorageKey,
    prev_state: Option<TrackState>,
    new_state: Option<TrackState>,
    value: Option<&StorageItem>,
) {
    if !is_watched_gas_balance_key(key) {
        return;
    }

    let context = current_storage_watch_context();
    let block_index = context.map(|ctx| ctx.block_index);
    let phase = context
        .map(|ctx| ctx.phase.as_str())
        .unwrap_or(StorageWatchPhase::Unknown.as_str());
    let tx_hash = context
        .and_then(|ctx| ctx.tx_hash)
        .map(|hash| hash.to_string())
        .unwrap_or_else(|| "<none>".to_string());
    let balance = value
        .map(|item| item.to_bigint().to_string())
        .unwrap_or_else(|| "<none>".to_string());

    info!(
        target: "neo",
        block_index = ?block_index,
        phase,
        tx_hash = %tx_hash,
        op,
        source,
        key_id = key.id(),
        key_prefix = key.key().first().copied().unwrap_or_default(),
        key_len = key.key().len(),
        prev_state = ?prev_state,
        new_state = ?new_state,
        balance = %balance,
        "watched DataCache key event"
    );
}

#[cfg(not(feature = "runtime"))]
pub(super) fn log_watched_storage_event(
    _op: &'static str,
    _source: &'static str,
    _key: &StorageKey,
    _prev_state: Option<TrackState>,
    _new_state: Option<TrackState>,
    _value: Option<&StorageItem>,
) {
}
