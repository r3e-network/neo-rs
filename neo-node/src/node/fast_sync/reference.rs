//! Reference-node checks for fast-sync completion.

use anyhow::Context;
use neo_io::{MemoryReader, Serializable};
use std::sync::Arc;

use super::local::LocalStateRootTip;
use super::package::FastSyncPackage;
use super::report::{FastSyncBlockReferenceProof, FastSyncStateRootReferenceProof};
use crate::node::rpc_payload::decode_remote_serialized_payload;

fn reference_rpc_client(endpoint: &str) -> anyhow::Result<neo_rpc::RpcClient> {
    let url = url::Url::parse(endpoint)
        .with_context(|| format!("invalid fast-sync reference RPC endpoint {endpoint:?}"))?;
    neo_rpc::RpcClient::builder(url)
        .build()
        .map_err(|err| anyhow::anyhow!("creating fast-sync reference RPC client: {err}"))
}

pub(super) async fn verify_block_tip(
    endpoint: &str,
    package: &FastSyncPackage,
    imported_tip: super::super::chain_acc::LocalLedgerTip,
) -> anyhow::Result<FastSyncBlockReferenceProof> {
    let client = reference_rpc_client(endpoint)?;
    let upstream_block = client
        .get_block_hex(&imported_tip.height.to_string())
        .await
        .map_err(|err| {
            anyhow::anyhow!(
                "fast-sync reference RPC getblock({}) raw failed for package {} at endpoint {}: {err}",
                imported_tip.height,
                package.filename,
                endpoint
            )
        })?;
    let upstream_block = decode_remote_serialized_payload(&upstream_block, "block", |bytes| {
        neo_payloads::Block::deserialize(&mut MemoryReader::new(bytes))
            .map_err(|err| anyhow::anyhow!("deserializing fast-sync reference block: {err}"))
    })
    .map_err(|err| {
        anyhow::anyhow!(
            "fast-sync reference RPC returned invalid raw block for height {} from package {}: {err}",
            imported_tip.height,
            package.filename
        )
    })?;
    let upstream_height = upstream_block.index();
    if upstream_height != imported_tip.height {
        anyhow::bail!(
            "fast-sync reference block height mismatch for package {}: imported height {}, upstream block height {}",
            package.filename,
            imported_tip.height,
            upstream_height
        );
    }
    let upstream_hash = upstream_block.try_hash().map_err(|err| {
        anyhow::anyhow!(
            "fast-sync reference block hash calculation failed for package {} at height {}: {err}",
            package.filename,
            imported_tip.height
        )
    })?;

    if upstream_hash != imported_tip.hash {
        anyhow::bail!(
            "fast-sync reference block hash mismatch for package {} at height {}: imported {}, upstream {}",
            package.filename,
            imported_tip.height,
            imported_tip.hash,
            upstream_hash
        );
    }

    Ok(FastSyncBlockReferenceProof {
        height: imported_tip.height,
        hash: upstream_hash,
    })
}

pub(super) async fn verify_state_root_tip(
    endpoint: &str,
    package: &FastSyncPackage,
    local_root: LocalStateRootTip,
) -> anyhow::Result<FastSyncStateRootReferenceProof> {
    let client = Arc::new(reference_rpc_client(endpoint)?);
    let upstream = neo_rpc::StateApi::new(client)
        .get_state_root(local_root.index)
        .await
        .map_err(|err| {
            anyhow::anyhow!(
                "fast-sync reference RPC getstateroot({}) failed for package {} at endpoint {}: {err}",
                local_root.index,
                package.filename,
                endpoint
            )
        })?;

    if upstream.index != local_root.index {
        anyhow::bail!(
            "fast-sync reference state root index mismatch for package {}: local {}, upstream {}",
            package.filename,
            local_root.index,
            upstream.index
        );
    }

    if upstream.root_hash != local_root.root_hash {
        anyhow::bail!(
            "fast-sync reference state root mismatch for package {} at height {}: local {}, upstream {}",
            package.filename,
            local_root.index,
            local_root.root_hash,
            upstream.root_hash
        );
    }

    Ok(FastSyncStateRootReferenceProof {
        height: upstream.index,
        root_hash: upstream.root_hash,
    })
}
