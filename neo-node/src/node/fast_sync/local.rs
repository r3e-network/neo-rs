//! Local-ledger verification for fast-sync imports.
//!
//! These checks protect the local canonical ledger before and after package
//! import: the package must start at the current durable tip, the imported tip
//! must match durable storage, and an enabled StateService must expose the same
//! local state-root tip before optional reference RPC verification runs.

use std::sync::Arc;

use neo_primitives::UInt256;
use neo_state_service::StateStore;
use neo_storage::persistence::store::Store;

use super::package::FastSyncPackage;

pub(super) fn validate_fast_sync_preflight<S>(
    store: &Arc<S>,
    package: &FastSyncPackage,
) -> anyhow::Result<()>
where
    S: Store,
{
    let durable_tip = super::super::chain_acc::local_ledger_tip(Some(store))?.map(|tip| tip.height);
    match package.start.checked_sub(1) {
        None => match durable_tip {
            None | Some(0) => Ok(()),
            Some(tip) if tip < package.end => Ok(()),
            Some(tip) if tip == package.end => Ok(()),
            Some(tip) => anyhow::bail!(
                "fast sync package {} ends at height {}, but local ledger is already at height {tip}; refusing to import over newer chain data",
                package.filename,
                package.end
            ),
        },
        Some(expected_tip) => match durable_tip {
            Some(tip) if tip == expected_tip => Ok(()),
            Some(tip) => anyhow::bail!(
                "fast sync package {} starts at height {}, but local ledger tip is {tip}; expected tip {expected_tip} before import",
                package.filename,
                package.start
            ),
            None => anyhow::bail!(
                "fast sync package {} starts at height {}, but local ledger has no tip; expected tip {expected_tip} before import",
                package.filename,
                package.start
            ),
        },
    }
}

pub(super) fn verify_fast_sync_import_tip<S>(
    store: &Arc<S>,
    package: &FastSyncPackage,
    report: &super::super::chain_acc::ChainAccImportReport,
) -> anyhow::Result<()>
where
    S: Store,
{
    let Some(imported_tip) = report.last_imported_tip else {
        if report.imported == 0 {
            return Ok(());
        }
        anyhow::bail!(
            "fast-sync package {} imported {} blocks but did not report a final block tip",
            package.filename,
            report.imported
        );
    };

    let durable_tip = super::super::chain_acc::local_ledger_tip(Some(store))?.ok_or_else(|| {
        anyhow::anyhow!(
            "fast-sync package {} imported to height {}, but local durable ledger has no tip after import",
            package.filename,
            imported_tip.height
        )
    })?;

    if durable_tip != imported_tip {
        anyhow::bail!(
            "fast-sync local ledger tip mismatch after package {}: expected imported tip height {} hash {}, local durable tip height {} hash {}",
            package.filename,
            imported_tip.height,
            imported_tip.hash,
            durable_tip.height,
            durable_tip.hash
        );
    }

    if imported_tip.height > package.end {
        anyhow::bail!(
            "fast-sync package {} imported tip height {} beyond package end {}",
            package.filename,
            imported_tip.height,
            package.end
        );
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LocalStateRootTip {
    pub(super) index: u32,
    pub(super) root_hash: UInt256,
}

pub(super) fn local_state_root_tip<S>(
    state_store: Option<&Arc<StateStore<S>>>,
    package: &FastSyncPackage,
    imported_tip: super::super::chain_acc::LocalLedgerTip,
) -> anyhow::Result<Option<LocalStateRootTip>>
where
    S: Store,
{
    let Some(state_store) = state_store else {
        return Ok(None);
    };
    let Some(mpt) = state_store.mpt() else {
        return Ok(None);
    };
    let Some((index, root_hash)) = mpt.current_local_root() else {
        anyhow::bail!(
            "fast-sync package {} imported to height {}, but StateService has no local state root",
            package.filename,
            imported_tip.height
        );
    };

    if index != imported_tip.height {
        anyhow::bail!(
            "fast-sync package {} local state-root tip height {} does not match imported block tip height {}",
            package.filename,
            index,
            imported_tip.height
        );
    }

    let state_root = mpt.get_state_root(imported_tip.height).ok_or_else(|| {
        anyhow::anyhow!(
            "fast-sync package {} has no local state root for imported tip height {}",
            package.filename,
            imported_tip.height
        )
    })?;

    if *state_root.root_hash() != root_hash {
        anyhow::bail!(
            "fast-sync package {} local state-root record mismatch at height {}: current root {}, indexed record {}",
            package.filename,
            imported_tip.height,
            root_hash,
            state_root.root_hash()
        );
    }

    Ok(Some(LocalStateRootTip { index, root_hash }))
}
